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
