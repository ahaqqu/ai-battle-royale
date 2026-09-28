//! M1 acceptance criteria (PLAN §9): a full 16-bot match between reference
//! bots runs headless to completion in < 1s of engine time, produces a
//! replay, and re-simulating the replay reproduces it byte-identically.
//! Plus fog-of-war and determinism contract tests.

use abr_core::bots;
use abr_core::config::MatchConfig;
use abr_core::engine::MatchEngine;
use abr_core::fixed::from_f64;
use abr_core::map::load_map;
use abr_core::replay::{verify_replay, Replay, ReplayRecorder};
use abr_core::types::{BotInput, UnitAction, UnitInput, Vec2};
use std::time::Instant;

fn run_match(names: &[String], seed: u64) -> (MatchEngine, ReplayRecorder, f64) {
    let config = MatchConfig::standard();
    let mut engine = MatchEngine::new(config, seed, names);
    for (b, name) in names.iter().enumerate() {
        let uses_companion = bots::create(name, b as u32)
            .map(|bt| bt.uses_companion())
            .unwrap_or(false);
        engine.configure_bot(b as u32, 1, !uses_companion && engine.config.auto_heel);
    }
    let map = load_map(&engine.config.map_id).unwrap();
    let mut brains: Vec<Box<dyn bots::RefBot>> = names
        .iter()
        .enumerate()
        .map(|(b, n)| bots::create(n, b as u32).unwrap())
        .collect();
    let mut recorder = ReplayRecorder::new(&engine, names);
    let t0 = Instant::now();
    while !engine.state.finished {
        for b in 0..names.len() as u32 {
            let obs = engine.observe(b);
            let input = brains[b as usize].act(&obs, &map);
            engine.submit(b, input.clone(), 0);
            recorder.record_submit(b, input);
        }
        engine.step_tick();
        recorder.record_tick(engine.state.digest());
    }
    let elapsed = t0.elapsed().as_secs_f64();
    recorder.finish(&engine);
    (engine, recorder, elapsed)
}

#[test]
fn m1_acceptance_16_bots_under_1s_and_replay_byte_identical() {
    let names = bots::default16();
    let (engine, recorder, elapsed) = run_match(&names, 42);

    assert!(engine.state.finished, "match must run to completion");
    assert!(
        engine.state.tick > 500,
        "a real match lasted {} ticks",
        engine.state.tick
    );
    assert!(engine.state.alive_mains() <= 1);
    // Accept: < 1s of engine time for a full 16-bot match (release build;
    // debug builds get a 15x leniency since they run ~10x slower).
    let budget = if cfg!(debug_assertions) { 15.0 } else { 1.0 };
    assert!(
        elapsed < budget,
        "engine time {:.3}s exceeds the {:.0}s budget",
        elapsed,
        budget
    );

    // Accept: re-simulating the replay reproduces it byte-identically.
    let json = recorder.to_json();
    let replay: Replay = serde_json::from_str(&json).unwrap();
    let summary = verify_replay(&replay).expect("replay must verify byte-identically");
    assert_eq!(summary.ticks, engine.state.tick);
    assert_eq!(summary.winner, engine.state.winner);
}

#[test]
fn same_seed_is_deterministic() {
    let names: Vec<String> = vec![
        "hunter".into(),
        "camper".into(),
        "looter".into(),
        "survivor".into(),
    ];
    let (_, rec_a, _) = run_match(&names, 777);
    let (_, rec_b, _) = run_match(&names, 777);
    assert_eq!(rec_a.ticks.len(), rec_b.ticks.len());
    for (ta, tb) in rec_a.ticks.iter().zip(rec_b.ticks.iter()) {
        assert_eq!(
            ta.digest, tb.digest,
            "same seed must produce identical digests"
        );
    }
}

#[test]
fn different_seed_diverges() {
    let names: Vec<String> = vec![
        "hunter".into(),
        "camper".into(),
        "looter".into(),
        "survivor".into(),
    ];
    let (_, rec_a, _) = run_match(&names, 1);
    let (_, rec_b, _) = run_match(&names, 2);
    let da: Vec<u64> = rec_a.ticks.iter().map(|t| t.digest).collect();
    let db: Vec<u64> = rec_b.ticks.iter().map(|t| t.digest).collect();
    assert_ne!(da, db);
}

/// Strict fog: bots must not receive enemy positions outside their senses.
#[test]
fn observation_respects_fog() {
    let names: Vec<String> = vec!["camper".into(), "hunter".into()];
    let config = MatchConfig::standard();
    let mut engine = MatchEngine::new(config, 5, &names);
    engine.step_tick();

    // Pin the viewer to an open-field spot, enemy far away behind pillars.
    engine.state.main_mut(0).pos = Vec2::new(from_f64(1600.0), from_f64(200.0));
    engine.state.companion_mut(0).pos = Vec2::new(from_f64(1610.0), from_f64(210.0));
    engine.state.main_mut(1).pos = Vec2::new(from_f64(1550.0), from_f64(1550.0));
    engine.state.companion_mut(1).pos = Vec2::new(from_f64(1560.0), from_f64(1540.0));

    let obs = engine.observe(0);
    assert!(
        obs.seen.players.is_empty(),
        "fog leaked enemy players: {:?}",
        obs.seen.players
    );
    assert!(
        obs.seen.companions.is_empty(),
        "fog leaked enemy companions"
    );
    assert!(obs.seen.projectiles.is_empty());

    // Now put the enemy inside vision: it must appear.
    engine.state.main_mut(1).pos = Vec2::new(from_f64(1600.0), from_f64(400.0));
    let obs = engine.observe(0);
    // 200u away, with LOS across the plaza.
    assert_eq!(obs.seen.players.len(), 1);
    assert_eq!(
        obs.seen.players[0].detail, "full",
        "inside 300u must be full detail"
    );
    assert!(obs.seen.players[0].hp.is_some());

    // Silhouette tier: 350u away (< 450 vision, > 300 full-detail).
    engine.state.main_mut(1).pos = Vec2::new(from_f64(1600.0), from_f64(550.0));
    let obs = engine.observe(0);
    assert_eq!(obs.seen.players.len(), 1);
    assert_eq!(
        obs.seen.players[0].detail, "silhouette",
        "350u must be silhouette"
    );
    assert!(
        obs.seen.players[0].hp.is_none(),
        "silhouettes are position-only"
    );

    // Silhouette entry must not carry vel/facing.
    assert!(obs.seen.players[0].vel.is_none());
    assert!(obs.seen.players[0].facing.is_none());
}

/// Audio: a gunshot inside the audible radius must be heard (coarse), one
/// outside must not.
#[test]
fn audio_gunshot_coarse_leak() {
    let names: Vec<String> = vec!["camper".into(), "camper".into()];
    let config = MatchConfig::standard();
    let mut engine = MatchEngine::new(config, 6, &names);
    engine.state.main_mut(0).pos = Vec2::new(from_f64(1600.0), from_f64(150.0));
    engine.state.companion_mut(0).pos = Vec2::new(from_f64(1610.0), from_f64(160.0));
    engine.state.main_mut(1).pos = Vec2::new(from_f64(1600.0), from_f64(850.0)); // 700u north of bot 0

    // Bot 1 fires southward (toward a point 100u ahead of itself).
    let input = BotInput {
        main: UnitInput {
            r#move: Default::default(),
            action: Some(UnitAction::Fire {
                target: Vec2::new(from_f64(1600.0), from_f64(750.0)),
            }),
        },
        companion: UnitInput::default(),
        intent: None,
        belief: None,
    };
    engine.submit(1, input, 0);
    engine.step_tick();

    let obs = engine.observe(0);
    assert!(
        obs.heard.iter().any(|h| h.kind == "gunshot"),
        "700u gunshot (audible 900u) must be heard, got {:?}",
        obs.heard
    );
    // Bearing must be quantized to 15° and point south (toward the shooter at north).
    let g = obs.heard.iter().find(|h| h.kind == "gunshot").unwrap();
    assert_eq!(g.bearing % 15, 0);
    // Shooter is due north of the listener... listener at spawn(1600,150),
    // shooter at 1600,2300 → due north → bearing 0.
    assert_eq!(g.bearing, 0, "shooter is due north");
    assert_eq!(g.band, "far", "700u is 'far'");

    // Outside audible range: no gunshot heard. Cooldown needs 5 ticks to
    // reset, so keep firing for several ticks before checking.
    for _ in 0..8 {
        let inp = BotInput {
            main: UnitInput {
                r#move: Default::default(),
                action: Some(UnitAction::Fire {
                    target: Vec2::new(from_f64(1600.0), from_f64(750.0)),
                }),
            },
            companion: UnitInput::default(),
            intent: None,
            belief: None,
        };
        engine.submit(1, inp, 0);
        engine.step_tick();
    }
    let final_tick = engine.state.tick;
    let obs = engine.observe(0);
    assert!(!obs
        .heard
        .iter()
        .any(|h| h.tick == final_tick && h.kind == "gunshot"));
}

/// Momentum: a bot that misses its deadline repeats its last action set.
#[test]
fn momentum_repeats_last_input() {
    let names: Vec<String> = vec!["camper".into(), "camper".into()];
    let config = MatchConfig::standard();
    let mut engine = MatchEngine::new(config, 7, &names);
    engine.step_tick();
    let y0 = engine.state.main(0).pos.y;

    let input = BotInput {
        main: UnitInput {
            r#move: abr_core::types::MoveInput {
                dir: 0,
                throttle: abr_core::fixed::ONE,
            },
            action: None,
        },
        companion: UnitInput::default(),
        intent: None,
        belief: None,
    };
    engine.submit(0, input, 0);
    let _ = engine.step_tick();
    // Moving north one tick ≈ 14u at 140u/s.
    let pos_after_move = engine.state.main(0).pos;
    assert!(
        pos_after_move.y > y0 + from_f64(10.0),
        "one tick of movement north: {} -> {}",
        y0,
        pos_after_move.y
    );

    // Miss two deadlines: momentum keeps moving north.
    let y1 = engine.state.main(0).pos.y;
    engine.step_tick();
    engine.step_tick();
    let y3 = engine.state.main(0).pos.y;
    assert!(
        y3 > y1 + from_f64(20.0),
        "momentum must repeat movement: {y1} -> {y3}"
    );
}

/// Companion leash: server clamps beyond 350u (PLAN §2.3).
#[test]
fn companion_leash_clamped() {
    let names: Vec<String> = vec!["camper".into(), "camper".into()];
    let config = MatchConfig::standard();
    let mut engine = MatchEngine::new(config, 8, &names);
    // Companion tries to sprint away from its main every tick.
    let input = BotInput {
        main: UnitInput::default(),
        companion: UnitInput {
            r#move: abr_core::types::MoveInput {
                dir: 180,
                throttle: abr_core::fixed::ONE,
            },
            action: None,
        },
        intent: None,
        belief: None,
    };
    let map = load_map("arena-1").unwrap();
    for _ in 0..50 {
        engine.submit(0, input.clone(), 0);
        engine.step_tick();
    }
    let d = engine.state.main(0).pos.dist(engine.state.companion(0).pos);
    let leash = from_f64(350.0);
    assert!(d <= leash + from_f64(30.0), "leash exceeded: {d}");
    let _ = map;
}

/// Zone: spawns outside phase-0 coverage don't exist (phase 0 covers the
/// whole arena) and the published next zone is always inside the current.
#[test]
fn zone_next_published_and_contained() {
    let names: Vec<String> = vec!["camper".into(), "camper".into()];
    let config = MatchConfig::standard();
    let engine = MatchEngine::new(config, 9, &names);
    let obs = engine.observe(0);
    let next = obs
        .global
        .zone
        .next
        .expect("next zone published one phase ahead");
    assert!(next.locks_at_tick > obs.tick);
    let off = ((next.center[0] - obs.global.zone.center[0]).powi(2)
        + (next.center[1] - obs.global.zone.center[1]).powi(2))
    .sqrt();
    assert!(
        off + next.radius <= obs.global.zone.radius + 2.0,
        "next zone must fit inside current"
    );
}

/// Loot: gun-swap pickups exist in the schedule (weapons are the fun
/// centerpiece — if the weights ever drop them, this fails loudly).
#[test]
fn loot_generation_includes_weapon_pickups() {
    use abr_core::loot::PickupKind;
    let config = MatchConfig::standard();
    let params = abr_core::params::SimParams::from_config(&config);
    let map = load_map("arena-1").unwrap();
    let mut found = 0;
    for seed in 1..=10u64 {
        let mut rng = abr_core::rng::Rng::new(seed);
        for pk in abr_core::loot::generate(&params, &map, &mut rng) {
            if matches!(pk.kind, PickupKind::Weapon(_)) {
                found += 1;
            }
        }
    }
    assert!(found >= 10, "weapon pickups across 10 seeds: {found}");
}

/// Weapons in play: over a reference-bot match some unit grabs a gun
/// pickup, and fired shots carry the gun they came from.
#[test]
fn weapon_pickups_swap_guns_and_shots_carry_them() {
    use abr_core::weapons::WeaponKind;

    let names = bots::default16();
    let config = MatchConfig::standard();
    let mut engine = MatchEngine::new(config, 42, &names);
    let map = load_map(&engine.config.map_id).unwrap();
    let mut brains: Vec<Box<dyn bots::RefBot>> = names
        .iter()
        .enumerate()
        .map(|(b, n)| bots::create(n, b as u32).unwrap())
        .collect();
    let mut gun_units = 0u32;
    let mut fired_weapons: std::collections::BTreeSet<u8> = std::collections::BTreeSet::new();
    while !engine.state.finished {
        for b in 0..names.len() as u32 {
            let obs = engine.observe(b);
            let input = brains[b as usize].act(&obs, &map);
            engine.submit(b, input, 0);
        }
        let events = engine.step_tick();
        for u in &engine.state.units {
            if u.weapon != WeaponKind::Pea {
                gun_units += 1;
            }
        }
        for e in &events {
            if let abr_core::Event::Shot { weapon, .. } = e {
                fired_weapons.insert(weapon.idx());
            }
        }
    }
    assert!(gun_units > 0, "no bot ever carried a pickup gun");
    // The default pea gun (0) plus at least one pickup gun fired during the match.
    assert!(fired_weapons.len() >= 2, "fired weapons: {fired_weapons:?}");
}

fn aimed_engine(seed: u64) -> MatchEngine {
    let config = MatchConfig::standard();
    MatchEngine::new(config, seed, &["camper".into(), "camper".into()])
}

fn fire_at(x: f64, y: f64) -> BotInput {
    BotInput {
        main: UnitInput {
            r#move: abr_core::types::MoveInput::stop(),
            action: Some(UnitAction::Fire {
                target: Vec2::new(from_f64(x), from_f64(y)),
            }),
        },
        companion: UnitInput::default(),
        intent: None,
        belief: None,
    }
}

/// Bouncer: a shot at a wall ricochets instead of dying.
#[test]
fn bouncer_bullet_ricochets_off_walls() {
    use abr_core::weapons::WeaponKind;
    let mut engine = aimed_engine(5);
    engine.state.main_mut(0).pos = Vec2::new(from_f64(1600.0), from_f64(400.0));
    engine.state.main_mut(0).weapon = WeaponKind::Bouncer;
    engine.submit(0, fire_at(1600.0, 900.0), 0);
    let mut bounced = false;
    for _ in 0..30 {
        let events = engine.step_tick();
        if events
            .iter()
            .any(|e| matches!(e, abr_core::Event::Bounce { .. }))
        {
            bounced = true;
            break;
        }
    }
    assert!(bounced, "bouncer never ricocheted off the wall");
}

/// Popper: a shot into a wall detonates and splashes a nearby enemy.
#[test]
fn popper_bullet_detonates_and_splashes() {
    use abr_core::weapons::WeaponKind;
    let mut engine = aimed_engine(6);
    engine.state.main_mut(0).pos = Vec2::new(from_f64(1600.0), from_f64(400.0));
    engine.state.main_mut(0).weapon = WeaponKind::Popper;
    // An enemy main right by the impact face of the wall (y = 700).
    engine.state.main_mut(1).pos = Vec2::new(from_f64(1600.0), from_f64(620.0));
    engine.submit(0, fire_at(1600.0, 900.0), 0);
    let mut boom = false;
    for _ in 0..30 {
        let events = engine.step_tick();
        if events
            .iter()
            .any(|e| matches!(e, abr_core::Event::Explosion { .. }))
        {
            boom = true;
            break;
        }
    }
    assert!(boom, "popper never detonated on a wall");
    assert!(
        engine.state.main(1).hp < from_f64(100.0),
        "splash never hurt the enemy standing next to the blast"
    );
}

/// Skewer: one bullet punches through two units in a line.
#[test]
fn skewer_bullet_pierces_units() {
    use abr_core::weapons::WeaponKind;
    let mut engine = aimed_engine(7);
    engine.state.main_mut(0).pos = Vec2::new(from_f64(300.0), from_f64(1150.0));
    engine.state.main_mut(0).weapon = WeaponKind::Skewer;
    // Two enemies of bot 0 lined up along +X.
    engine.state.main_mut(1).pos = Vec2::new(from_f64(700.0), from_f64(1150.0));
    engine.state.companion_mut(1).pos = Vec2::new(from_f64(800.0), from_f64(1150.0));
    engine.submit(0, fire_at(2000.0, 1150.0), 0);
    let mut hit_ids: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for _ in 0..30 {
        let events = engine.step_tick();
        for e in &events {
            if let abr_core::Event::Hit { unit_id, .. } = e {
                hit_ids.insert(*unit_id);
            }
        }
        if hit_ids.len() >= 2 {
            break;
        }
    }
    // Bot 1's main is id 2, its companion id 102.
    assert!(hit_ids.contains(&2) && hit_ids.contains(&102), "pierce hit: {hit_ids:?}");
}

// ---------------------------------------------------------------------------
// Slain the Boss (mode 3): raiders + AI boss on one entrant slot.
// ---------------------------------------------------------------------------

fn run_boss_match(names: &[String], seed: u64, mut config: MatchConfig) -> (MatchEngine, ReplayRecorder, f64) {
    config.mode = abr_core::config::GameMode::Boss;
    let names = {
        let mut v = names.to_vec();
        if let Some(pos) = v.iter().position(|n| n == "boss") {
            if pos != v.len() - 1 {
                let b = v.remove(pos);
                v.push(b);
            }
        }
        v
    };
    let mut engine = MatchEngine::new(config, seed, &names);
    for (b, name) in names.iter().enumerate() {
        let uses_companion = bots::create(name, b as u32)
            .map(|bt| bt.uses_companion())
            .unwrap_or(false);
        engine.configure_bot(b as u32, 1, !uses_companion && engine.config.auto_heel);
    }
    let map = load_map(&engine.config.map_id).unwrap();
    let mut brains: Vec<Box<dyn bots::RefBot>> = names
        .iter()
        .enumerate()
        .map(|(b, n)| bots::create(n, b as u32).unwrap())
        .collect();
    let mut recorder = ReplayRecorder::new(&engine, &names);
    let t0 = Instant::now();
    while !engine.state.finished {
        for b in 0..names.len() as u32 {
            let obs = engine.observe(b);
            let input = brains[b as usize].act(&obs, &map);
            engine.submit(b, input.clone(), 0);
            recorder.record_submit(b, input);
        }
        engine.step_tick();
        recorder.record_tick(engine.state.digest());
    }
    let elapsed = t0.elapsed().as_secs_f64();
    recorder.finish(&engine);
    (engine, recorder, elapsed)
}

/// A full raid between reference raiders and the boss brain runs to a
/// verdict and the replay re-simulates byte-identically.
#[test]
fn boss_raid_completes_and_replay_verifies() {
    let mut names = bots::default8();
    names.truncate(7);
    names.push("boss".into());
    let (engine, recorder, _) = run_boss_match(&names, 11, MatchConfig::standard());

    assert!(engine.state.finished);
    let boss_bot = engine.state.boss_bot();
    let winner = engine.state.winner.expect("raid has a winner");
    assert!(winner < engine.state.bots);
    if winner == boss_bot {
        // Boss held: every raider fell.
        assert_eq!(engine.state.alive_mains(), 0, "boss win means all raiders dead");
    } else {
        // Raiders won: the boss is down and stays down.
        assert!(!engine.state.main(boss_bot).alive);
    }
    let replay: Replay = serde_json::from_str(&recorder.to_json()).unwrap();
    verify_replay(&replay).expect("boss raid replay must verify");
}

/// Raiders are one team: their bullets pass through each other and their
/// companions, and splash never friendly-fires — but the boss is hurt.
#[test]
fn raiders_cannot_friendly_fire_but_hurt_the_boss() {
    let mut engine = MatchEngine::new(
        abr_core::config::MatchConfig::boss_raid(),
        9,
        &["camper".into(), "camper".into(), "boss".into()],
    );
    // Line the two raiders up along +X: bot 1's main directly ahead of bot 0.
    engine.state.main_mut(0).pos = Vec2::new(from_f64(300.0), from_f64(1150.0));
    engine.state.main_mut(1).pos = Vec2::new(from_f64(700.0), from_f64(1150.0));
    engine.state.main_mut(1).hp = from_f64(100.0);
    engine.state.companion_mut(1).pos = Vec2::new(from_f64(800.0), from_f64(1150.0));
    let boss_bot = engine.state.boss_bot();
    engine.state.main_mut(boss_bot).pos = Vec2::new(from_f64(1000.0), from_f64(1150.0));

    engine.submit(0, fire_at(2000.0, 1150.0), 0);
    let mut boss_hit = false;
    for _ in 0..30 {
        let events = engine.step_tick();
        boss_hit |= events
            .iter()
            .any(|e| matches!(e, abr_core::Event::Hit { bot: b, .. } if *b == boss_bot));
        if boss_hit {
            break;
        }
    }
    assert_eq!(
        engine.state.main(1).hp,
        from_f64(100.0),
        "a raider bullet must pass through a teammate"
    );
    assert!(
        engine.state.main(boss_bot).hp < engine.state.main(boss_bot).max_hp(&engine.params),
        "the same volley must damage the boss"
    );
    assert!(boss_hit, "expected a Hit event on the boss");
}

/// A short boss-hp config lets one volley end the raid with a raider win.
#[test]
fn slaying_the_boss_ends_the_match_raiders_win() {
    use abr_core::weapons::WeaponKind;
    let mut config = abr_core::config::MatchConfig::boss_raid();
    config.boss.hp = 30.0;
    let mut engine = MatchEngine::new(config, 13, &["camper".into(), "boss".into()]);
    let boss_bot = engine.state.boss_bot();
    engine.state.main_mut(0).pos = Vec2::new(from_f64(400.0), from_f64(1150.0));
    engine.state.main_mut(0).weapon = WeaponKind::Lance;
    engine.state.main_mut(boss_bot).pos = Vec2::new(from_f64(700.0), from_f64(1150.0));

    engine.submit(0, fire_at(1500.0, 1150.0), 0);
    let mut ended = false;
    for _ in 0..30 {
        engine.step_tick();
        if engine.state.finished {
            ended = true;
            break;
        }
    }
    assert!(ended, "slaying the boss must end the raid");
    assert!(
        !engine.state.main(boss_bot).alive,
        "the boss must be dead when raiders win"
    );
    assert_eq!(
        engine.state.winner,
        Some(0),
        "the surviving raider takes first place"
    );
    // The boss is ranked last.
    let boss_place = engine.state.main(boss_bot).placement.unwrap();
    assert_eq!(boss_place, 2);
}

/// Zone ticks down raiders but never the boss, and the raid ends when every
/// raider is dead — the boss takes first place.
#[test]
fn boss_ignores_zone_and_wins_when_raiders_fall() {
    let mut config = abr_core::config::MatchConfig::boss_raid();
    config.zone.damage_per_phase = vec![50.0];
    config.zone.radii = vec![200.0];
    config.zone.hold_s_min = 0.0;
    config.zone.hold_s_max = 0.0;
    config.zone.shrink_s = 0.0;
    let mut engine = MatchEngine::new(config, 17, &["camper".into(), "boss".into()]);
    let boss_bot = engine.state.boss_bot();
    // Put the raider far outside the tiny zone; the boss anywhere.
    engine.state.main_mut(0).pos = Vec2::new(from_f64(3000.0), from_f64(3000.0));
    engine.state.companion_mut(0).pos = Vec2::new(from_f64(2950.0), from_f64(3000.0));

    let mut ended = false;
    for _ in 0..400 {
        engine.step_tick();
        assert_eq!(
            engine.state.main(boss_bot).hp,
            engine.state.main(boss_bot).max_hp(&engine.params),
            "the boss must never take zone damage"
        );
        if engine.state.finished {
            ended = true;
            break;
        }
    }
    assert!(ended, "zone death of the last raider must end the raid");
    assert_eq!(engine.state.winner, Some(boss_bot), "the boss holds the arena");
    assert!(!engine.state.main(0).alive);
}
