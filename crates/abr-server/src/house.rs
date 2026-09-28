//! House bots: in-process reference brains (PLAN §10 "reference bot tiers")
//! wired to the exact same channels a remote WS bot uses. The queue spawns
//! them to top a match up to 8 entrants whenever a human is waiting, so a
//! solo player gets a real match in seconds without hosting anything.
//! House bots are stored in the DB (placements need a bot id) but excluded
//! from the standings — they are sparring partners, not competitors.

use crate::{BotHandle, BotMsg};
use abr_core::bots;
use abr_core::map::GameMap;
use abr_core::observe::Observation;
use abr_core::types::BotInput;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex};

static HOUSE_SEQ: AtomicU64 = AtomicU64::new(1);

/// Spawn one house bot running `brain_name`. Its task reads the same JSON
/// the gateway writes to remote bots and writes back the same action shape,
/// so the match loop cannot tell it apart from a networked bot.
pub fn spawn(db: &crate::db::Db, brain_name: &str) -> Arc<BotHandle> {
    let seq = HOUSE_SEQ.fetch_add(1, Ordering::Relaxed);
    let name = format!("house·{}·{:04}", brain_name, seq % 10000);
    let db_id = db.register_bot(&name, "").unwrap_or(0);
    let mut brain = bots::create(brain_name, (seq % 16) as u32).expect("known reference brain");
    let uses_companion = brain.uses_companion();

    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);
    let (in_tx, in_rx) = mpsc::channel::<BotMsg>(64);
    let connected = Arc::new(AtomicBool::new(true));
    let connected2 = connected.clone();
    tokio::spawn(async move {
        let mut map: Option<GameMap> = None;
        while let Some(text) = out_rx.recv().await {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            match v["type"].as_str() {
                Some("match_start") => {
                    let map_id = v["map_id"].as_str().unwrap_or("arena-1");
                    map = abr_core::map::load_map(map_id).or_else(|| map.take());
                }
                Some("match_over") => break,
                _ => {
                    let Ok(obs) = serde_json::from_value::<Observation>(v) else {
                        continue;
                    };
                    if let Some(m) = &map {
                        let input: BotInput = brain.act(&obs, m);
                        let _ = in_tx
                            .send(BotMsg {
                                client_tick: obs.tick,
                                input,
                                arrived: Instant::now(),
                            })
                            .await;
                    }
                }
            }
        }
        connected2.store(false, Ordering::Relaxed);
    });

    Arc::new(BotHandle {
        name,
        db_id,
        decision_rate: 1,
        // Brains that ignore their companion get the server-side auto-heel AI,
        // same as a remote bot that registers auto_heel (PLAN §2.3).
        auto_heel: !uses_companion,
        human: false,
        house: true,
        mode: abr_core::config::GameMode::Royale,
        wants_boss: false,
        lobby: std::sync::Mutex::new(None),
        connected,
        out_tx,
        in_rx: Arc::new(Mutex::new(in_rx)),
    })
}

/// Spawn the built-in raid boss for a Slain-the-Boss match with no
/// registered boss AI. Same plumbing as a house bot; named (and hidden from
/// the standings) like one too.
pub fn spawn_boss(db: &crate::db::Db) -> Arc<BotHandle> {
    let seq = HOUSE_SEQ.fetch_add(1, Ordering::Relaxed);
    let name = format!("house·boss·{:04}", seq % 10000);
    let db_id = db.register_bot(&name, "").unwrap_or(0);
    let mut brain = bots::create("boss", (seq % 16) as u32).expect("boss brain");
    let uses_companion = brain.uses_companion();

    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);
    let (in_tx, in_rx) = mpsc::channel::<BotMsg>(64);
    let connected = Arc::new(AtomicBool::new(true));
    let connected2 = connected.clone();
    tokio::spawn(async move {
        let mut map: Option<GameMap> = None;
        while let Some(text) = out_rx.recv().await {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            match v["type"].as_str() {
                Some("match_start") => {
                    let map_id = v["map_id"].as_str().unwrap_or("arena-1");
                    map = abr_core::map::load_map(map_id).or_else(|| map.take());
                }
                Some("match_over") => break,
                _ => {
                    let Ok(obs) = serde_json::from_value::<Observation>(v) else {
                        continue;
                    };
                    if let Some(m) = &map {
                        let input: BotInput = brain.act(&obs, m);
                        let _ = in_tx
                            .send(BotMsg {
                                client_tick: obs.tick,
                                input,
                                arrived: Instant::now(),
                            })
                            .await;
                    }
                }
            }
        }
        connected2.store(false, Ordering::Relaxed);
    });

    Arc::new(BotHandle {
        name,
        db_id,
        decision_rate: 1,
        auto_heel: !uses_companion,
        human: false,
        house: true,
        mode: abr_core::config::GameMode::Boss,
        wants_boss: true,
        lobby: std::sync::Mutex::new(None),
        connected,
        out_tx,
        in_rx: Arc::new(Mutex::new(in_rx)),
    })
}

/// The roster a solo human faces: a spread over all six tiers, aggression
/// included — same lineup spirit as `default8` (human + 7).
pub const HOUSE_ROSTER: &[&str] = &[
    "hunter",
    "looter",
    "camper",
    "survivor",
    "wanderer",
    "berserker",
    "hunter",
];
