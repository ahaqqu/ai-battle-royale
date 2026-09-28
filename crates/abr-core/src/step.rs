//! The tick pipeline. Resolution order is part of the published rules
//! (PLAN §5.1): movement → dashes → projectile spawns → projectile flight +
//! impacts → ability effects → zone damage → pickups → deaths. Deterministic
//! by construction: fixed-point math, sorted iteration, seeded RNG only.
//!
//! `inputs` is indexed per **unit**: `[bot*2]` = main, `[bot*2 + 1]` =
//! companion. The engine fills these (momentum, auto-heel) before calling.

use crate::events::{pa, Event};
use crate::fixed::{self, Fix, ONE};
use crate::loot::PickupKind;
use crate::map::GameMap;
use crate::params::{lerp_point, SimParams};
use crate::state::{
    apply_damage, KillEntry, Projectile, SonarActive, SoundEvent, SoundKind, Unit, WorldState,
};
use crate::types::{MoveInput, UnitAction, UnitInput, Vec2};
use crate::zone;

const MAX_PROJECTILES: usize = 900;
/// Damage must land within this many ticks of death for kill credit;
/// stale damage never steals a zone kill.
const KILL_CREDIT_WINDOW_TICKS: u64 = 50;

pub fn step(
    state: &mut WorldState,
    p: &SimParams,
    map: &GameMap,
    inputs: &[Option<UnitInput>],
) -> Vec<Event> {
    let mut events: Vec<Event> = Vec::new();
    state.sounds.clear();
    state.tick += 1;
    let dt = p.dt;
    let tick = state.tick;

    let prev_zone = zone::zone_at(&state.zone_phases, tick - 1);
    let cur_zone = zone::zone_at(&state.zone_phases, tick);

    // ------------------------------------------------- 1. movement
    // Pass A: compute the tick's motion for every unit (read-only, so
    // companions can see their main's fresh position for heel/leash).
    struct Motion {
        vel: Vec2,
        facing: u16,
        footsteps: bool,
    }
    let mut motions: Vec<Motion> = Vec::with_capacity(state.units.len());
    for (ui, unit) in state.units.iter().enumerate() {
        if !unit.alive {
            motions.push(Motion {
                vel: Vec2::default(),
                facing: unit.facing,
                footsteps: false,
            });
            continue;
        }
        let is_main = unit.is_main();
        let base_speed = unit.base_speed(p);
        let input = &inputs[ui];
        let mut move_in: MoveInput = input.map(|i| i.r#move).unwrap_or_default();
        let acting = input.and_then(|i| i.action);

        // Companion `heel`: server-side return-to-main override (PLAN §2.3).
        if !is_main && acting == Some(UnitAction::Heel) {
            let main_pos = state.main(unit.bot).pos;
            let d = main_pos.dist(unit.pos);
            if d > fixed::from_int(100) {
                move_in = MoveInput {
                    dir: fixed::norm_deg(unit.pos.bearing_to(main_pos)),
                    throttle: ONE,
                };
            } else {
                move_in = MoveInput::stop();
            }
        }

        let (vel, facing) = if unit.is_combatant() && unit.dashing > 0 {
            let v = unit.dash_dir.scale(fixed::mul(base_speed, p.dash_mult));
            let f = fixed::norm_deg(fixed::atan2_deg(unit.dash_dir.y, unit.dash_dir.x));
            (v, f)
        } else if move_in.throttle > 0 {
            let mut s = fixed::mul(base_speed, move_in.throttle);
            if is_main && unit.sprint {
                s = fixed::mul(s, p.sprint_mult);
            }
            if unit.shielding > 0 {
                s = fixed::mul(s, p.shield_speed);
            }
            (Vec2::dir(move_in.dir as i32).scale(s), move_in.dir)
        } else {
            (Vec2::default(), unit.facing)
        };

        motions.push(Motion {
            vel,
            facing,
            footsteps: is_main && unit.sprint && move_in.throttle > 0,
        });
    }
    // Pass B: integrate into a position buffer first (mains index before
    // their companion, so the leash sees the main's fresh position), then
    // write everything back.
    let mut newpos: Vec<Vec2> = state.units.iter().map(|u| u.pos).collect();
    for (ui, unit) in state.units.iter().enumerate() {
        if !unit.alive {
            continue;
        }
        let m = &motions[ui];
        let rad = unit.radius(p);
        let mut pos = unit.pos;
        if m.vel != Vec2::default() {
            pos = pos.add(m.vel.scale(dt));
            crate::map::resolve_circle(map, &mut pos, rad);
        }
        if !unit.is_main() {
            let main_idx = (unit.bot * 2) as usize;
            let main_pos = newpos[main_idx];
            let d2 = pos.dist2(main_pos);
            if d2 > fixed::mul(p.leash, p.leash) && d2 > 0 {
                let d = fixed::sqrt(d2);
                let target = main_pos.add(pos.sub(main_pos).scale(fixed::div(p.leash, d)));
                pos = target;
                crate::map::resolve_circle(map, &mut pos, rad);
            }
        }
        newpos[ui] = pos;
    }
    for (ui, unit) in state.units.iter_mut().enumerate() {
        let m = &motions[ui];
        unit.vel = m.vel;
        unit.facing = m.facing;
        unit.pos = newpos[ui];
    }
    for (ui, unit) in state.units.iter().enumerate() {
        if unit.alive && motions[ui].footsteps {
            state.sounds.push(SoundEvent {
                kind: SoundKind::Footstep,
                pos: unit.pos,
                bot: unit.bot,
            });
        }
    }

    // ------------------------------------------------- 2. unit separation
    let n = state.units.len();
    for i in 0..n {
        if !state.units[i].alive {
            continue;
        }
        for j in (i + 1)..n {
            if !state.units[j].alive {
                continue;
            }
            let min = state.units[i].radius(p) + state.units[j].radius(p);
            let a = state.units[i].pos;
            let b = state.units[j].pos;
            let d2 = a.dist2(b);
            if d2 < fixed::mul(min, min) && d2 > 0 {
                let d = fixed::sqrt(d2);
                let push = (min - d) / 2;
                let nx = fixed::div(b.x - a.x, d);
                let ny = fixed::div(b.y - a.y, d);
                state.units[i].pos.x -= fixed::mul(nx, push);
                state.units[i].pos.y -= fixed::mul(ny, push);
                state.units[j].pos.x += fixed::mul(nx, push);
                state.units[j].pos.y += fixed::mul(ny, push);
            }
        }
    }

    // ------------------------------------------------- 3. dashes
    for (ui, unit) in state.units.iter_mut().enumerate() {
        if !unit.alive || !unit.is_combatant() || unit.dashing > 0 {
            continue;
        }
        if inputs[ui].and_then(|i| i.action) == Some(UnitAction::Dash) && unit.energy >= p.dash_cost
        {
            unit.energy -= p.dash_cost;
            let mv = inputs[ui].map(|i| i.r#move).unwrap_or_default();
            let dir = if mv.throttle > 0 {
                Vec2::dir(mv.dir as i32)
            } else {
                Vec2::dir(unit.facing as i32)
            };
            unit.dash_dir = dir;
            unit.dashing = p.dash_ticks;
            state.sounds.push(SoundEvent {
                kind: SoundKind::Dash,
                pos: unit.pos,
                bot: unit.bot,
            });
            events.push(Event::Dash {
                bot: unit.bot,
                unit_id: unit.id,
                at: pa(unit.pos),
                dir: unit.facing,
            });
        }
    }

    // ------------------------------------------------- 4. projectile spawns (fire)
    for (ui, unit) in state.units.iter_mut().enumerate() {
        if !unit.alive || !unit.is_combatant() {
            continue;
        }
        if let Some(UnitAction::Fire { target }) = inputs[ui].and_then(|i| i.action) {
            // Sprint disables firing (PLAN §2.2).
            if unit.sprint || unit.fire_cd > 0 || state.projectiles.len() >= MAX_PROJECTILES {
                continue;
            }
            let spec = crate::weapons::spec(p, unit.weapon);
            unit.fire_cd = effective_fire_cooldown(p, unit);
            let d = target.sub(unit.pos);
            let dir_deg = if d.x == 0 && d.y == 0 {
                unit.facing as i32
            } else {
                fixed::atan2_deg(d.y, d.x)
            };
            unit.facing = fixed::norm_deg(dir_deg);
            let speed = projectile_speed(p, unit);
            // Pellet fan (shotgun) centered on the aim, plus per-shot wobble
            // from the seeded RNG — deterministic across re-simulation.
            let n = spec.pellets.max(1);
            let mut pid = state.next_projectile_id;
            for i in 0..n {
                let fan = if n > 1 && spec.spread_deg > 0 {
                    -spec.spread_deg / 2 + (i as i32) * (spec.spread_deg / (n as i32 - 1))
                } else {
                    0
                };
                let jitter = if spec.jitter_deg > 0 {
                    (state.rng.below(2 * spec.jitter_deg as u64 + 1) as i32) - spec.jitter_deg
                } else {
                    0
                };
                let deg = fixed::norm_deg(dir_deg + fan + jitter) as i32;
                let dirv = Vec2::dir(deg);
                let spawn = unit
                    .pos
                    .add(dirv.scale(unit.radius(p) + fixed::from_int(4)));
                state.projectiles.push(Projectile {
                    id: pid,
                    bot: unit.bot,
                    unit_id: unit.id,
                    pos: spawn,
                    vel: dirv.scale(speed),
                    damage: spec.damage,
                    remaining: spec.range,
                    weapon: unit.weapon,
                    bounces: spec.bounces,
                    pierce_left: spec.pierce,
                    hits: [0; 4],
                    hits_n: 0,
                });
                pid += 1;
            }
            state.next_projectile_id = pid;
            state.sounds.push(SoundEvent {
                kind: SoundKind::Gunshot,
                pos: unit.pos,
                bot: unit.bot,
            });
            events.push(Event::Shot {
                bot: unit.bot,
                unit_id: unit.id,
                from: pa(unit.pos),
                dir: unit.facing,
                weapon: unit.weapon,
            });
        }
    }

    // ------------------------------------------------- 5. projectile flight + impacts
    let mut survivors: Vec<Projectile> = Vec::with_capacity(state.projectiles.len());
    let projectiles = std::mem::take(&mut state.projectiles);
    for mut proj in projectiles {
        let spec = crate::weapons::spec(p, proj.weapon);
        let speed = proj.vel.len();
        let old = proj.pos;
        let new = old.add(proj.vel.scale(dt));
        let mut dead = false;

        if let Some((t, nx, ny)) = crate::map::projectile_wall_hit_n(map, old, new) {
            let at = lerp_point(old, new, t);
            if proj.bounces > 0 {
                // Ricochet: reflect the velocity about the wall normal and
                // nudge off the surface so we don't re-hit it next tick.
                let dot = fixed::mul(proj.vel.x, nx) + fixed::mul(proj.vel.y, ny);
                proj.vel = Vec2::new(
                    proj.vel.x - fixed::mul(2 * dot, nx),
                    proj.vel.y - fixed::mul(2 * dot, ny),
                );
                proj.pos = at.add(Vec2::new(nx, ny).scale(fixed::from_int(2)));
                proj.bounces -= 1;
                proj.remaining -= fixed::mul(spec.range, fixed::from_f64(0.15));
                events.push(Event::Bounce {
                    id: proj.id,
                    at: [fixed::to_f64(at.x), fixed::to_f64(at.y)],
                });
                if proj.remaining > 0 {
                    survivors.push(proj);
                    continue;
                }
                events.push(Event::ProjectileEnd {
                    id: proj.id,
                    at: [fixed::to_f64(proj.pos.x), fixed::to_f64(proj.pos.y)],
                    wall: true,
                });
                dead = true;
            } else {
                if spec.splash_radius > 0 {
                    explode(state, &mut events, at, proj.bot, spec, p, tick, None);
                }
                events.push(Event::ProjectileEnd {
                    id: proj.id,
                    at: [fixed::to_f64(at.x), fixed::to_f64(at.y)],
                    wall: true,
                });
                dead = true;
            }
        } else {
            // Earliest *not-already-hit* unit along the sweep. Teammates are
            // immune — the companion is a bullet sponge *for enemies*, not
            // for you (and in boss mode raiders never friendly-fire).
            let mut best: Option<(Fix, usize)> = None;
            for (ui, unit) in state.units.iter().enumerate() {
                if !unit.alive || state.same_team(unit.bot, proj.bot, p.mode) {
                    continue;
                }
                if proj.hits[..proj.hits_n as usize].contains(&unit.id) {
                    continue;
                }
                let r = unit.radius(p) + fixed::from_int(2);
                if let Some(t) = segment_circle_t(old, new, unit.pos, r) {
                    if best.is_none_or(|(bt, _)| t < bt) {
                        best = Some((t, ui));
                    }
                }
            }
            if let Some((t, ui)) = best {
                let at = lerp_point(old, new, t);
                let (vid, vbot, vhp, dmg) = {
                    let victim = &mut state.units[ui];
                    let dmg = apply_damage(victim, proj.damage, p.shield_reduction);
                    if victim.is_main() {
                        victim.last_damager = Some(proj.bot);
                        victim.last_damager_tick = tick;
                    }
                    (victim.id, victim.bot, victim.hp, dmg)
                };
                state.damage_dealt[proj.bot as usize] += dmg.max(0) as u64;
                events.push(Event::Hit {
                    unit_id: vid,
                    bot: vbot,
                    at: [fixed::to_f64(at.x), fixed::to_f64(at.y)],
                    damage: fixed::to_f64(dmg) as f32,
                    hp_after: fixed::to_f64(vhp) as f32,
                });
                if spec.splash_radius > 0 {
                    explode(state, &mut events, at, proj.bot, spec, p, tick, Some(ui));
                }
                // Skewer: record the hit and keep flying.
                if proj.pierce_left > 0 {
                    proj.hits[proj.hits_n as usize % proj.hits.len()] = vid;
                    proj.hits_n = (proj.hits_n + 1).min(proj.hits.len() as u8);
                    proj.pierce_left -= 1;
                    proj.pos = at;
                    survivors.push(proj);
                    continue;
                }
                dead = true;
            } else {
                proj.pos = new;
                proj.remaining -= fixed::mul(speed, dt);
                if proj.remaining <= 0 {
                    if spec.splash_radius > 0 {
                        explode(state, &mut events, proj.pos, proj.bot, spec, p, tick, None);
                    }
                    events.push(Event::ProjectileEnd {
                        id: proj.id,
                        at: [fixed::to_f64(proj.pos.x), fixed::to_f64(proj.pos.y)],
                        wall: false,
                    });
                    dead = true;
                }
            }
        }
        if !dead {
            survivors.push(proj);
        }
    }
    state.projectiles = survivors;

    // ------------------------------------------------- 6. ability effects
    for (ui, unit) in state.units.iter_mut().enumerate() {
        if !unit.alive {
            continue;
        }
        match inputs[ui].and_then(|i| i.action) {
            Some(UnitAction::Shield) if unit.is_combatant() => {
                if unit.shielding == 0 && unit.energy >= p.shield_cost {
                    unit.energy -= p.shield_cost;
                    unit.shielding = p.shield_ticks;
                    events.push(Event::Shield {
                        bot: unit.bot,
                        unit_id: unit.id,
                        at: pa(unit.pos),
                    });
                }
            }
            Some(UnitAction::Sprint { on }) if unit.is_main() => {
                unit.sprint = on;
            }
            Some(UnitAction::Sonar)
                if !unit.is_main() && unit.sonar_cd <= 0 && unit.energy >= p.sonar_cost =>
            {
                unit.energy -= p.sonar_cost;
                unit.sonar_cd = p.sonar_cooldown;
                state.sonars.push(SonarActive {
                    bot: unit.bot,
                    pos: unit.pos,
                    until_tick: tick + p.sonar_ticks,
                });
                state.sounds.push(SoundEvent {
                    kind: SoundKind::Sonar,
                    pos: unit.pos,
                    bot: unit.bot,
                });
                events.push(Event::Sonar {
                    bot: unit.bot,
                    unit_id: unit.id,
                    at: pa(unit.pos),
                });
            }
            _ => {}
        }
    }

    // ------------------------------------------------- 7. zone damage
    {
        let zn = cur_zone;
        for unit in state.units.iter_mut() {
            if !unit.alive {
                continue;
            }
            // The boss IS the endgame — it never burns to the zone.
            if unit.is_boss() {
                continue;
            }
            if unit.pos.dist(zn.center) > zn.radius {
                unit.hp -= fixed::mul(zn.damage, dt);
            }
        }
    }

    // ------------------------------------------------- 8. pickups (mains only)
    for pi in 0..state.pickups.len() {
        let (taken, spawn_tick, kind, ppos, pid) = {
            let pk = &state.pickups[pi];
            (pk.taken, pk.spawn_tick, pk.kind, pk.pos, pk.id)
        };
        if taken || tick < spawn_tick {
            continue;
        }
        for ui in 0..state.units.len() {
            let (ualive, umain, upos, ubot, urad) = {
                let unit = &state.units[ui];
                (
                    unit.alive,
                    unit.is_main(),
                    unit.pos,
                    unit.bot,
                    unit.radius(p),
                )
            };
            if !ualive || !umain {
                continue;
            }
            if upos.dist(ppos) <= p.pickup_radius + urad {
                apply_pickup(state, ui, kind, p);
                state.pickups[pi].taken = true;
                events.push(Event::Pickup {
                    bot: ubot,
                    pickup_id: pid,
                    kind,
                    at: pa(ppos),
                });
                break;
            }
        }
    }

    // ------------------------------------------------- 9. regen + cooldowns
    for unit in state.units.iter_mut() {
        if !unit.alive {
            continue;
        }
        if !unit.sprint && unit.dashing == 0 && unit.shielding == 0 {
            unit.energy = (unit.energy + fixed::mul(p.energy_regen, dt)).min(unit.max_energy(p));
        }
        unit.fire_cd = (unit.fire_cd - dt).max(0.into());
        unit.sonar_cd = (unit.sonar_cd - dt).max(0.into());
        unit.dashing = unit.dashing.saturating_sub(1);
        unit.shielding = unit.shielding.saturating_sub(1);
    }

    // ------------------------------------------------- 10. deaths
    let alive_before = state.alive_mains();
    let died: Vec<usize> = state
        .units
        .iter()
        .enumerate()
        .filter(|(_, u)| u.alive && u.hp <= 0)
        .map(|(i, _)| i)
        .collect();
    for ui in died {
        let (eliminated, bot, pos, killer, uid) = {
            let unit = &state.units[ui];
            let killer = if unit.is_combatant() {
                unit.last_damager
                    .filter(|_| tick - unit.last_damager_tick <= KILL_CREDIT_WINDOW_TICKS)
            } else {
                None
            };
            (unit.is_combatant(), unit.bot, unit.pos, killer, unit.id)
        };
        state.units[ui].alive = false;
        if eliminated {
            state.units[ui].placement = Some(alive_before);
            if let Some(k) = killer {
                state.main_mut(k).kills += 1;
            }
            state.kill_feed.push(KillEntry {
                tick,
                killer,
                victim: bot,
            });
            events.push(Event::Death {
                bot,
                unit_id: uid,
                at: pa(pos),
                killer,
            });
            // Elimination takes the companion with it.
            let comp = state.companion_mut(bot);
            if comp.alive {
                comp.alive = false;
                comp.respawn_at = None;
            }
        } else {
            state.units[ui].respawn_at = Some(tick + p.respawn_ticks);
            events.push(Event::CompanionDown {
                bot,
                unit_id: uid,
                at: pa(pos),
            });
        }
    }

    // ------------------------------------------------- 11. companion respawn
    let mut respawns: Vec<u32> = state
        .units
        .iter()
        .filter(|u| !u.is_main() && !u.alive && u.respawn_at == Some(tick))
        .map(|u| u.bot)
        .collect();
    respawns.sort_unstable();
    respawns.dedup();
    for bot in respawns {
        let main_alive = state.main(bot).alive;
        if !main_alive {
            continue;
        }
        let main_pos = state.main(bot).pos;
        let comp_rad = state.companion(bot).radius(p);
        let comp = state.companion_mut(bot);
        comp.alive = true;
        comp.hp = comp.max_hp(p);
        comp.energy = comp.max_energy(p);
        comp.pos = main_pos.add(Vec2::new(fixed::from_int(30), fixed::from_int(0)));
        crate::map::resolve_circle(map, &mut comp.pos, comp_rad);
        comp.respawn_at = None;
        let cid = comp.id;
        let cpos = comp.pos;
        events.push(Event::CompanionBack {
            bot,
            unit_id: cid,
            at: pa(cpos),
        });
    }

    // Expire sonars.
    state.sonars.retain(|s| s.until_tick > tick);

    // ------------------------------------------------- 12. zone phase events
    if cur_zone.phase != prev_zone.phase
        && tick >= cur_lock_tick(&state.zone_phases, cur_zone.phase)
    {
        events.push(Event::ZoneLocked {
            phase: cur_zone.phase,
        });
    }
    if cur_zone.phase + 1 < state.zone_phases.len() {
        let nxt = &state.zone_phases[cur_zone.phase + 1];
        if tick == nxt.shrink_start_tick {
            events.push(Event::ZoneShrinkStarted {
                phase: cur_zone.phase + 1,
                center: pa(nxt.center),
                radius: fixed::to_f64(nxt.radius) as f32,
            });
        }
    }

    // ------------------------------------------------- 13. win check
    if p.mode == crate::config::GameMode::Boss {
        finish_boss_raid(state, tick, p.match_max_ticks, &mut events);
    } else {
        let alive_now = state.alive_mains();
        if !state.finished && (alive_now <= 1 || tick >= p.match_max_ticks) {
            state.finished = true;
            let mut survivors: Vec<&Unit> = state
                .units
                .iter()
                .filter(|u| u.is_main() && u.alive)
                .collect();
            // hp desc, kills desc, bot asc — documented deterministic tie-break.
            survivors.sort_by(|a, b| {
                b.hp.cmp(&a.hp)
                    .then(b.kills.cmp(&a.kills))
                    .then(a.bot.cmp(&b.bot))
            });
            for u in survivors {
                state.placements.push(u.bot);
            }
            for (rank, b) in state.placements.clone().iter().enumerate() {
                state.main_mut(*b).placement = Some(rank as u32 + 1);
            }
            state.winner = state.placements.first().copied();
            events.push(Event::MatchEnded {
                winner: state.winner,
            });
        }
    }

    events
}

/// Slain-the-Boss endgame: the raid ends when the boss falls (raiders win,
/// ranked hp → kills → bot; the boss is last) or when every raider is dead
/// or the clock runs out (boss holds the arena — boss wins, first place).
fn finish_boss_raid(state: &mut WorldState, tick: u64, max_ticks: u64, events: &mut Vec<Event>) {
    if state.finished {
        return;
    }
    let boss_bot = state.boss_bot();
    let boss_alive = state.main(boss_bot).alive;
    let raiders_alive = state.alive_mains();
    if boss_alive && raiders_alive > 0 && tick < max_ticks {
        return;
    }
    state.finished = true;
    // Surviving raiders ranked hp desc, kills desc, bot asc; dead raiders in
    // reverse death order (their placement at death, ascending); boss last
    // when slain, first when it holds.
    let mut survivors: Vec<&Unit> = state
        .units
        .iter()
        .filter(|u| u.is_main() && u.alive)
        .collect();
    survivors.sort_by(|a, b| {
        b.hp.cmp(&a.hp)
            .then(b.kills.cmp(&a.kills))
            .then(a.bot.cmp(&b.bot))
    });
    let mut fallen: Vec<u32> = state
        .units
        .iter()
        .filter(|u| u.is_main() && !u.alive && u.bot != boss_bot)
        .map(|u| u.bot)
        .collect();
    fallen.sort_by_key(|b| state.main(*b).placement.unwrap_or(u32::MAX));
    let boss_won = boss_alive;
    if boss_won {
        state.placements.push(boss_bot);
    }
    for u in survivors {
        state.placements.push(u.bot);
    }
    for b in fallen {
        state.placements.push(b);
    }
    if !boss_won {
        state.placements.push(boss_bot);
    }
    for (rank, b) in state.placements.clone().iter().enumerate() {
        state.main_mut(*b).placement = Some(rank as u32 + 1);
    }
    state.winner = state.placements.first().copied();
    events.push(Event::MatchEnded {
        winner: state.winner,
    });
}

fn cur_lock_tick(phases: &[crate::zone::ZonePhase], phase: usize) -> u64 {
    phases.get(phase).map(|ph| ph.lock_tick).unwrap_or(0)
}

/// t in [0,1] where segment a→b first comes within `r` of circle center c.
fn segment_circle_t(a: Vec2, b: Vec2, c: Vec2, r: Fix) -> Option<Fix> {
    let d = b.sub(a);
    let f = a.sub(c);
    let qa = fixed::mul(d.x, d.x) + fixed::mul(d.y, d.y);
    let qc = f.len2() - fixed::mul(r, r);
    if qa == 0 {
        return None;
    }
    let qb = 2 * (fixed::mul(f.x, d.x) + fixed::mul(f.y, d.y));
    let disc = fixed::mul(qb, qb) - 4 * fixed::mul(qa, qc);
    if disc < 0 {
        return None;
    }
    let sq = fixed::sqrt(disc);
    let denom = 2 * qa;
    let t = fixed::div(-qb - sq, denom);
    if (0..=ONE).contains(&t) {
        return Some(t);
    }
    let t2 = fixed::div(-qb + sq, denom);
    if (0..=ONE).contains(&t2) {
        return Some(t2);
    }
    None
}

fn apply_pickup(state: &mut WorldState, ui: usize, kind: PickupKind, p: &SimParams) {
    let u = &mut state.units[ui];
    match kind {
        PickupKind::HpKit => u.hp = (u.hp + p.hp_kit).min(p.main_hp),
        PickupKind::Energy => u.energy = (u.energy + p.energy_pack).min(p.energy_max),
        PickupKind::ModCooldown => {
            u.mod_cooldown_pct =
                (-p.mod_cooldown_pct_max).max(u.mod_cooldown_pct - p.mod_cooldown_pct)
        }
        PickupKind::ModSpeed => {
            u.mod_speed_pct = p.mod_speed_pct_max.min(u.mod_speed_pct + p.mod_speed_pct)
        }
        PickupKind::Weapon(w) => u.weapon = w,
    }
}

/// Popper detonation: splash every enemy near `at`. The direct-hit victim
/// (`skip`) already took the full payload and is not splashed again.
#[allow(clippy::too_many_arguments)]
fn explode(
    state: &mut WorldState,
    events: &mut Vec<Event>,
    at: Vec2,
    bot: u32,
    spec: crate::weapons::WeaponSpec,
    p: &SimParams,
    tick: u64,
    skip: Option<usize>,
) {
    events.push(Event::Explosion {
        bot,
        at: [fixed::to_f64(at.x), fixed::to_f64(at.y)],
        radius: fixed::to_f64(spec.splash_radius) as f32,
    });
    let mode = p.mode;
    let boss_bot = state.boss_bot();
    let friendly = |b: u32| b == bot || (mode == crate::config::GameMode::Boss && b != boss_bot);
    for (ui, unit) in state.units.iter_mut().enumerate() {
        if Some(ui) == skip || !unit.alive || friendly(unit.bot) {
            continue;
        }
        if unit.pos.dist(at) > spec.splash_radius + unit.radius(p) {
            continue;
        }
        let dmg = apply_damage(unit, spec.splash_damage, p.shield_reduction);
        state.damage_dealt[bot as usize] += dmg.max(0) as u64;
        if unit.is_main() {
            unit.last_damager = Some(bot);
            unit.last_damager_tick = tick;
        }
        events.push(Event::Hit {
            unit_id: unit.id,
            bot: unit.bot,
            at: [fixed::to_f64(at.x), fixed::to_f64(at.y)],
            damage: fixed::to_f64(dmg) as f32,
            hp_after: fixed::to_f64(unit.hp) as f32,
        });
    }
}

pub fn effective_fire_cooldown(p: &SimParams, u: &Unit) -> Fix {
    // mod_cooldown_pct is negative; cooldown shrinks toward its cap.
    let base = crate::weapons::spec(p, u.weapon).cooldown;
    let cd = base + fixed::mul(base, u.mod_cooldown_pct);
    cd.max(fixed::from_f64(0.05))
}

pub fn projectile_speed(p: &SimParams, u: &Unit) -> Fix {
    let base = crate::weapons::spec(p, u.weapon).speed;
    base + fixed::mul(base, u.mod_speed_pct)
}
