//! Strict fog of war (PLAN §3): `observe(state, viewer)` is a pure function
//! over the full state. What the server never sends stays out by
//! construction — no ghosts, no seed, no schedules. Everything here
//! serializes to the wire JSON of PLAN §3.6.

use crate::fixed::{self, Fix};
use crate::params::SimParams;
use crate::state::{SoundKind, WorldState};
use crate::types::Vec2;
use serde::{Deserialize, Serialize};

fn f(v: Fix) -> f64 {
    fixed::to_f64(v)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Observation {
    pub apiversion: u32,
    pub tick: u64,
    pub deadline_ms: u64,
    pub you: You,
    pub seen: Seen,
    pub heard: Vec<Heard>,
    pub global: Global,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct You {
    pub main: OwnUnit,
    pub companion: OwnUnit,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OwnUnit {
    pub id: u32,
    pub alive: bool,
    pub pos: [f64; 2],
    pub vel: [f64; 2],
    pub facing: u16,
    pub hp: f64,
    pub energy: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mods: Option<Mods>,
    pub cooldown: OwnCooldown,
    pub status: Vec<String>,
    /// Companion only: seconds until respawn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub respawn_in_s: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Mods {
    pub fire_cooldown_pct: i32,
    pub projectile_speed_pct: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OwnCooldown {
    pub fire: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sonar: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Seen {
    pub players: Vec<SeenPlayer>,
    pub companions: Vec<SeenCompanion>,
    pub projectiles: Vec<SeenProjectile>,
    pub pickups: Vec<SeenPickup>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SeenPlayer {
    pub id: u32,
    pub pos: [f64; 2],
    pub range: f64,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vel: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facing: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hp: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via_sonar: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SeenCompanion {
    pub id: u32,
    pub owner: u32,
    pub pos: [f64; 2],
    pub range: f64,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via_sonar: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SeenProjectile {
    pub id: u32,
    pub pos: [f64; 2],
    pub vel: [f64; 2],
    pub owner: u32,
    pub owner_kind: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SeenPickup {
    pub id: u32,
    pub pos: [f64; 2],
    pub kind: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Heard {
    pub tick: u64,
    pub kind: String,
    /// Degrees from north, quantized (15°).
    pub bearing: u16,
    pub band: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Global {
    pub bots: u32,
    pub alive: u32,
    pub kill_feed: Vec<KillEntryJson>,
    pub zone: ZoneJson,
    pub map_id: String,
    pub match_time_left_s: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KillEntryJson {
    pub tick: u64,
    pub killer: Option<u32>,
    pub victim: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ZoneJson {
    pub center: [f64; 2],
    pub radius: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<NextZone>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NextZone {
    pub center: [f64; 2],
    pub radius: f64,
    pub locks_at_tick: u64,
}

struct Sensed {
    idx: usize,
    pos: Vec2,
    radius: Fix,
}

pub fn observe(
    state: &WorldState,
    p: &SimParams,
    map: &crate::map::GameMap,
    viewer: u32,
) -> Observation {
    let main = state.main(viewer);
    let comp = state.companion(viewer);

    // --- sensing units (union of both circles, PLAN §3.2)
    let mut sensors: Vec<Sensed> = Vec::with_capacity(2);
    if main.alive {
        sensors.push(Sensed {
            idx: 0,
            pos: main.pos,
            radius: p.main_vision,
        });
    }
    if comp.alive {
        sensors.push(Sensed {
            idx: 1,
            pos: comp.pos,
            radius: p.comp_vision,
        });
    }
    let listener = if main.alive {
        main.pos
    } else if comp.alive {
        comp.pos
    } else {
        main.pos
    };

    // --- seen: players
    let mut seen = Seen::default();
    for (ui, u) in state.units.iter().enumerate() {
        if u.bot == viewer || !u.alive {
            continue;
        }
        let is_main = u.is_main();
        // Vision: nearest sensor with LOS.
        let mut best: Option<(Fix, usize)> = None;
        for s in &sensors {
            let d = u.pos.dist(s.pos);
            if d <= s.radius
                && !crate::map::los_blocked(map, s.pos, u.pos)
                && best.is_none_or(|(bd, _)| d < bd)
            {
                best = Some((d, s.idx));
            }
        }
        // Sonar reveal: silhouettes through walls, within reveal radius of
        // one of MY active sonar pings (PLAN §2.3).
        let mut sonar_range: Option<Fix> = None;
        for son in &state.sonars {
            if son.bot == viewer {
                let d = u.pos.dist(son.pos);
                if d <= p.sonar_reveal {
                    sonar_range = Some(sonar_range.map_or(d, |prev: Fix| prev.min(d)));
                }
            }
        }

        let detail_full = best.is_some_and(|(d, _)| d <= p.full_detail_range);
        if best.is_none() && sonar_range.is_none() {
            continue;
        }
        let range = match best {
            Some((d, _)) => d,
            None => sonar_range.expect("visible by sonar"),
        };
        if is_main {
            seen.players.push(SeenPlayer {
                id: u.id,
                pos: [f(u.pos.x), f(u.pos.y)],
                range: f(range),
                detail: if detail_full {
                    "full".to_string()
                } else {
                    "silhouette".to_string()
                },
                vel: detail_full.then(|| [f(u.vel.x), f(u.vel.y)]),
                facing: detail_full.then_some(u.facing),
                hp: detail_full.then(|| f(u.hp)),
                status: detail_full.then(|| unit_status(u)),
                via_sonar: (best.is_none()).then_some(true),
            });
        } else {
            seen.companions.push(SeenCompanion {
                id: u.id,
                owner: u.bot,
                pos: [f(u.pos.x), f(u.pos.y)],
                range: f(range),
                detail: if detail_full {
                    "full".to_string()
                } else {
                    "silhouette".to_string()
                },
                via_sonar: (best.is_none()).then_some(true),
            });
        }
        let _ = ui;
    }

    // --- seen: projectiles (own always; enemy only inside vision, PLAN §3.2)
    for pr in &state.projectiles {
        if pr.bot == viewer {
            seen.projectiles.push(SeenProjectile {
                id: pr.id,
                pos: [f(pr.pos.x), f(pr.pos.y)],
                vel: [f(pr.vel.x), f(pr.vel.y)],
                owner: pr.bot,
                owner_kind: "main".to_string(),
            });
            continue;
        }
        for s in &sensors {
            let d = pr.pos.dist(s.pos);
            if d <= s.radius && !crate::map::los_blocked(map, s.pos, pr.pos) {
                seen.projectiles.push(SeenProjectile {
                    id: pr.id,
                    pos: [f(pr.pos.x), f(pr.pos.y)],
                    vel: [f(pr.vel.x), f(pr.vel.y)],
                    owner: pr.bot,
                    owner_kind: "main".to_string(),
                });
                break;
            }
        }
    }

    // --- seen: pickups (static objects, need LOS)
    for pk in &state.pickups {
        if pk.taken || state.tick < pk.spawn_tick {
            continue;
        }
        for s in &sensors {
            let d = pk.pos.dist(s.pos);
            if d <= s.radius && !crate::map::los_blocked(map, s.pos, pk.pos) {
                seen.pickups.push(SeenPickup {
                    id: pk.id,
                    pos: [f(pk.pos.x), f(pk.pos.y)],
                    kind: pickup_kind_str(pk.kind).to_string(),
                });
                break;
            }
        }
    }

    // --- heard: audio events (coarse: kind + bearing + band, PLAN §3.2)
    let mut heard = Vec::new();
    for snd in &state.sounds {
        if snd.bot == viewer {
            continue;
        }
        let audible = match snd.kind {
            SoundKind::Gunshot => p.audio_gunshot,
            SoundKind::Dash => p.audio_dash,
            SoundKind::Footstep => p.audio_footstep,
            SoundKind::Sonar => p.sonar_audio,
        };
        let d = snd.pos.dist(listener);
        if d > audible {
            continue;
        }
        let bearing_raw = listener.bearing_to(snd.pos);
        let q = p.bearing_q as i32;
        let bearing = (((bearing_raw.rem_euclid(360) + q / 2) / q) * q) % 360;
        heard.push(Heard {
            tick: state.tick,
            kind: sound_kind_str(snd.kind).to_string(),
            bearing: bearing as u16,
            band: band_str(d).to_string(),
        });
        if heard.len() >= 64 {
            break;
        }
    }

    // --- global
    let zn = crate::zone::zone_at(&state.zone_phases, state.tick);
    let next = crate::zone::next_phase(&state.zone_phases, state.tick);
    let feed_start = state.kill_feed.len().saturating_sub(32);
    let global = Global {
        bots: state.bots,
        alive: state.alive_mains(),
        kill_feed: state.kill_feed[feed_start..]
            .iter()
            .map(|k| KillEntryJson {
                tick: k.tick,
                killer: k.killer,
                victim: k.victim,
            })
            .collect(),
        zone: ZoneJson {
            center: [f(zn.center.x), f(zn.center.y)],
            radius: f(zn.radius),
            next: next.map(|n| NextZone {
                center: [f(n.center.x), f(n.center.y)],
                radius: f(n.radius),
                locks_at_tick: n.lock_tick,
            }),
        },
        map_id: map.id.clone(),
        match_time_left_s: f(
            fixed::from_int(p.match_max_ticks as i64) / fixed::from_int(p.tick_rate as i64)
        ) - (state.tick as f64 / p.tick_rate as f64),
    };

    let you = You {
        main: own_unit(main, p, false, state.tick),
        companion: own_unit(comp, p, true, state.tick),
    };

    Observation {
        apiversion: 1,
        tick: state.tick,
        deadline_ms: p.deadline_ms,
        you,
        seen,
        heard,
        global,
    }
}

fn own_unit(u: &crate::state::Unit, p: &SimParams, companion: bool, tick: u64) -> OwnUnit {
    let status = unit_status(u);
    OwnUnit {
        id: u.id,
        alive: u.alive,
        pos: [f(u.pos.x), f(u.pos.y)],
        vel: [f(u.vel.x), f(u.vel.y)],
        facing: u.facing,
        hp: f(u.hp),
        energy: f(u.energy),
        mods: if companion {
            None
        } else if u.mod_cooldown_pct != 0 || u.mod_speed_pct != 0 {
            Some(Mods {
                fire_cooldown_pct: (f(u.mod_cooldown_pct) * 100.0).round() as i32,
                projectile_speed_pct: (f(u.mod_speed_pct) * 100.0).round() as i32,
            })
        } else {
            None
        },
        cooldown: if companion {
            OwnCooldown {
                fire: 0.0,
                sonar: Some(f(u.sonar_cd)),
            }
        } else {
            OwnCooldown {
                fire: f(u.fire_cd),
                sonar: None,
            }
        },
        status,
        respawn_in_s: if companion && !u.alive {
            u.respawn_at
                .map(|t| (t.saturating_sub(tick)) as f64 / p.tick_rate as f64)
        } else {
            None
        },
    }
}

fn unit_status(u: &crate::state::Unit) -> Vec<String> {
    let mut v = Vec::with_capacity(3);
    if u.sprint {
        v.push("sprint".to_string());
    }
    if u.dashing > 0 {
        v.push("dashing".to_string());
    }
    if u.shielding > 0 {
        v.push("shielding".to_string());
    }
    v
}

fn sound_kind_str(k: SoundKind) -> &'static str {
    match k {
        SoundKind::Gunshot => "gunshot",
        SoundKind::Dash => "dash",
        SoundKind::Footstep => "footstep",
        SoundKind::Sonar => "sonar",
    }
}

fn band_str(d: Fix) -> &'static str {
    let d = f(d);
    if d < 250.0 {
        "near"
    } else if d < 600.0 {
        "mid"
    } else {
        "far"
    }
}

fn pickup_kind_str(k: crate::loot::PickupKind) -> &'static str {
    match k {
        crate::loot::PickupKind::HpKit => "hp_kit",
        crate::loot::PickupKind::Energy => "energy",
        crate::loot::PickupKind::ModCooldown => "mod_cooldown",
        crate::loot::PickupKind::ModSpeed => "mod_speed",
    }
}

// ---------------------------------------------------------------------------
// Spectator frame: the FULL state, everything the bots don't get (PLAN §6.1).
// This is what live viewers and replays render.
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpectatorFrame {
    pub tick: u64,
    pub units: Vec<SpecUnit>,
    pub projectiles: Vec<SeenProjectile>,
    pub pickups: Vec<SeenPickup>,
    pub zone: ZoneJson,
    pub events: Vec<crate::events::Event>,
    pub alive: u32,
    pub kill_feed: Vec<KillEntryJson>,
    pub finished: bool,
    pub winner: Option<u32>,
    /// Mind-cam debug channel per bot: {intent, belief 64x64}, write-only.
    pub minds: std::collections::BTreeMap<u32, MindView>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpecUnit {
    pub id: u32,
    pub bot: u32,
    pub kind: String,
    pub pos: [f64; 2],
    pub vel: [f64; 2],
    pub facing: u16,
    pub hp: f64,
    pub max_hp: f64,
    pub energy: f64,
    pub alive: bool,
    pub sprint: bool,
    pub dashing: bool,
    pub shielding: bool,
    pub kills: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MindView {
    pub intent: Option<String>,
    /// 64×64 = 4096 bytes, row-major from north-west.
    pub belief: Option<Vec<u8>>,
}

pub fn spectator_frame(
    state: &WorldState,
    p: &SimParams,
    events: &[crate::events::Event],
    minds: &std::collections::BTreeMap<u32, (Option<String>, Option<Vec<u8>>)>,
) -> SpectatorFrame {
    let zn = crate::zone::zone_at(&state.zone_phases, state.tick);
    let next = crate::zone::next_phase(&state.zone_phases, state.tick);
    let feed_start = state.kill_feed.len().saturating_sub(64);
    SpectatorFrame {
        tick: state.tick,
        units: state
            .units
            .iter()
            .map(|u| SpecUnit {
                id: u.id,
                bot: u.bot,
                kind: if u.is_main() {
                    "main".to_string()
                } else {
                    "companion".to_string()
                },
                pos: [f(u.pos.x), f(u.pos.y)],
                vel: [f(u.vel.x), f(u.vel.y)],
                facing: u.facing,
                hp: f(u.hp),
                max_hp: f(u.max_hp(p)),
                energy: f(u.energy),
                alive: u.alive,
                sprint: u.sprint,
                dashing: u.dashing > 0,
                shielding: u.shielding > 0,
                kills: u.kills,
            })
            .collect(),
        projectiles: state
            .projectiles
            .iter()
            .map(|pr| SeenProjectile {
                id: pr.id,
                pos: [f(pr.pos.x), f(pr.pos.y)],
                vel: [f(pr.vel.x), f(pr.vel.y)],
                owner: pr.bot,
                owner_kind: "main".to_string(),
            })
            .collect(),
        pickups: state
            .pickups
            .iter()
            .filter(|pk| !pk.taken && state.tick >= pk.spawn_tick)
            .map(|pk| SeenPickup {
                id: pk.id,
                pos: [f(pk.pos.x), f(pk.pos.y)],
                kind: pickup_kind_str(pk.kind).to_string(),
            })
            .collect(),
        zone: ZoneJson {
            center: [f(zn.center.x), f(zn.center.y)],
            radius: f(zn.radius),
            next: next.map(|n| NextZone {
                center: [f(n.center.x), f(n.center.y)],
                radius: f(n.radius),
                locks_at_tick: n.lock_tick,
            }),
        },
        events: events.to_vec(),
        alive: state.alive_mains(),
        kill_feed: state.kill_feed[feed_start..]
            .iter()
            .map(|k| KillEntryJson {
                tick: k.tick,
                killer: k.killer,
                victim: k.victim,
            })
            .collect(),
        finished: state.finished,
        winner: state.winner,
        minds: minds
            .iter()
            .map(|(b, (intent, belief))| {
                (
                    *b,
                    MindView {
                        intent: intent.clone(),
                        belief: belief.clone(),
                    },
                )
            })
            .collect(),
    }
}
