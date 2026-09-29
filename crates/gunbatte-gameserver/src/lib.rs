//! Game-server role (AGENTS.md): everything inside one match. The matchmaker
//! hands over a roster of entrants plus a [`MatchContext`]; this side runs
//! the 10Hz loop, pushes one strict-fog observation per tick, collects one
//! action reply per entrant (50ms deadline, momentum on misses), records the
//! replay, and writes results + ELO back through the database. It never
//! touches the queue, lobbies, or identity state — one match, one owner.

use gunbatte_core::config::{GameMode, MatchConfig};
use gunbatte_core::engine::MatchEngine;
use gunbatte_core::replay::{build_summary, ReplayRecorder};
use gunbatte_node::db;
use gunbatte_node::{BotMsg, MatchContext, MatchEntrant};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Consecutive dropped observations before a non-reading bot is moved onto
/// the disconnect path (a healthy socket drains its channel continuously).
const STALL_LIMIT: u32 = 10;

/// What a finished match reports back to whoever hosted it.
pub struct MatchOutcome {
    pub replay_url: String,
    pub winner: Option<String>,
    pub ticks: u64,
}

/// The match seed is the one secret of a match — it reproduces the zone
/// schedule and every loot spawn (PLAN §5.1) — so it comes from the OS
/// CSPRNG, not from wall-clock time, which every participant knows.
fn random_seed() -> u64 {
    let mut buf = [0u8; 8];
    match getrandom::getrandom(&mut buf) {
        Ok(()) => u64::from_le_bytes(buf) | 1,
        Err(_) => {
            // No OS RNG available: degrade to the old time-based seed.
            let d = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            (d.subsec_nanos() as u64 ^ d.as_secs()) | 1
        }
    }
}

/// Run one match to completion: the 10Hz match loop (PLAN §4.2) with obs at
/// t=0, 50ms reply deadline, ~50ms resolution window, simultaneous
/// resolution. `config` decides the mode — royale drafts pass the server
/// default, raids pass a Boss-mode clone. Push this tick's observations.
/// Never awaits a bot's channel: a bot that stopped reading would otherwise
/// stall the whole lane once its bounded channel fills, so a full slot drops
/// the frame instead, and consecutive drops move the bot onto the disconnect
/// path (grace, then forfeit).
///
/// Returns when the match is over and persisted; requeueing survivors is
/// the matchmaker's job, not this function's.
pub async fn run_match(ctx: MatchContext, entrants: Vec<MatchEntrant>, config: MatchConfig) -> MatchOutcome {
    let n = entrants.len();
    let names: Vec<String> = entrants.iter().map(|h| h.name.clone()).collect();
    let seed = random_seed();
    let mut engine = MatchEngine::new(config.clone(), seed, &names);
    for (b, h) in entrants.iter().enumerate() {
        engine.configure_bot(b as u32, h.decision_rate, h.auto_heel);
    }
    let mut recorder = ReplayRecorder::new(&engine, &names);
    let boss_bot = if config.mode == GameMode::Boss {
        Some(n - 1)
    } else {
        None
    };

    // Tell the bots what they're in for. Best effort: a stalled socket must
    // not wedge the match before it begins.
    for (b, h) in entrants.iter().enumerate() {
        let _ = h
            .out_tx
            .try_send(
                serde_json::json!({
                    "type": "match_start",
                    "bot": h.name,
                    "bots": names,
                    "map_id": engine.config.map_id,
                    "deadline_ms": engine.config.deadline_ms,
                    "tick_rate": engine.config.tick_rate_hz,
                    "seedless": true,
                    "mode": if config.mode == GameMode::Boss { "boss" } else { "royale" },
                    "role": if Some(b) == boss_bot { "boss" } else { "raider" },
                })
                .to_string(),
            );
    }
    println!(
        "▶ match started: {} entrants: {}{}",
        n,
        names.join(", "),
        if boss_bot.is_some() {
            format!("  [SLAIN THE BOSS — boss: {}]", names[n - 1])
        } else {
            String::new()
        }
    );

    let tick_ms: u64 = 1000 / config.tick_rate_hz.max(1) as u64;
    let deadline = Duration::from_millis(config.deadline_ms);
    let mut interval = tokio::time::interval(Duration::from_millis(tick_ms));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut disconnected = vec![false; n];
    let mut send_stalls = vec![0u32; n];

    while !engine.state.finished {
        interval.tick().await;
        let tick_start = Instant::now();

        // Push this tick's observation to every bot simultaneously.
        push_observations(&mut engine, &entrants, &mut send_stalls);

        // Reply deadline window (PLAN §4.2): anything arriving within the
        // deadline is on time; the tail of the window catches stragglers,
        // whose true latency feeds the timeout ladder (§4.3).
        tokio::time::sleep(deadline).await;
        drain_inputs(
            &mut engine,
            &mut recorder,
            &entrants,
            tick_start,
            &mut disconnected,
        )
        .await;
        tokio::time::sleep(Duration::from_millis(
            tick_ms
                .saturating_sub(config.deadline_ms)
                .max(1)
                .saturating_sub(2),
        ))
        .await;
        drain_inputs(
            &mut engine,
            &mut recorder,
            &entrants,
            tick_start,
            &mut disconnected,
        )
        .await;

        let events = engine.step_tick();
        recorder.record_tick(engine.state.digest());

        // Spectate bus: full state, everything the bots don't get. The sink
        // comes from the matchmaker via the context; nobody here knows how
        // (or whether) spectators are being served.
        let frame = engine.spectator_frame(&events);
        if let Ok(json) = serde_json::to_string(&serde_json::json!({
            "type": "frame",
            "frame": frame,
        })) {
            let _ = ctx.spectate.send(json);
        }
    }

    // Persist: replay file + ladder + ELO — the game role's output contract.
    recorder.finish(&engine);
    let summary = build_summary(&engine, &names);
    let match_seq = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let file_name = format!("match-{match_seq}.json");
    let path = ctx.replay_dir.join(&file_name);
    std::fs::write(&path, recorder.to_json()).ok();
    let replay_url = format!("/replays/{file_name}");

    let places: std::collections::HashMap<String, i64> = summary
        .placements
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.clone(), i as i64 + 1))
        .collect();
    let ratings: Vec<(String, i64)> = names
        .iter()
        .map(|nm| (nm.clone(), ctx.db.elo_of(nm)))
        .collect();
    let elos = db::elo_update(&ratings, &places, 32.0);
    let results: Vec<(String, i64, i64)> = summary
        .placements
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.clone(), i as i64 + 1, p.kills as i64))
        .collect();
    ctx.db
        .record_match(seed, n as i64, summary.ticks, &replay_url, &results, &elos);
    println!(
        "✔ match over in {} ticks — winner: {:?} — replay: {}",
        summary.ticks, summary.winner, replay_url
    );

    // Notify the bots. Best effort: the lane must free up even if a bot
    // stopped reading. (Requeueing connected survivors back into the public
    // queue is matchmaking work and happens on the other side of the seam.)
    for (b, h) in entrants.iter().enumerate() {
        let place = summary
            .placements
            .iter()
            .position(|p| p.bot == b as u32)
            .map(|i| i + 1)
            .unwrap_or(0);
        let _ = h
            .out_tx
            .try_send(
                serde_json::json!({
                    "type": "match_over",
                    "place": place,
                    "replay": replay_url,
                    "new_elo": elos.iter().find(|(nm, _, _)| nm == &h.name).map(|(_, _, e)| *e),
                })
                .to_string(),
            );
    }

    MatchOutcome {
        replay_url,
        // The summary names the winner by bot index; report the name.
        winner: summary.winner.map(|w| names[w as usize].clone()),
        ticks: summary.ticks,
    }
}

/// Push this tick's observation to every entrant simultaneously.
fn push_observations(engine: &mut MatchEngine, entrants: &[MatchEntrant], stalls: &mut [u32]) {
    for (b, h) in entrants.iter().enumerate() {
        let obs = engine.observe(b as u32);
        let json = serde_json::to_string(&obs).unwrap_or_default();
        match h.out_tx.try_send(json) {
            Ok(()) => stalls[b] = 0,
            Err(mpsc::error::TrySendError::Full(_)) => {
                stalls[b] += 1;
                if stalls[b] == STALL_LIMIT {
                    println!("⏸ {} stopped reading observations — disconnect grace", h.name);
                    engine.disconnect(b as u32, engine.state.tick);
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                engine.disconnect(b as u32, engine.state.tick);
            }
        }
    }
}

/// Collect each entrant's newest message in the window and submit it.
async fn drain_inputs(
    engine: &mut MatchEngine,
    recorder: &mut ReplayRecorder,
    entrants: &[MatchEntrant],
    tick_start: Instant,
    disconnected: &mut [bool],
) {
    for (b, h) in entrants.iter().enumerate() {
        let mut rx = h.in_rx.lock().await;
        // Coalesce: each submit overwrites the bot's pending slot, so only
        // the newest message in the window can matter — a flooding bot costs
        // one parse, not a burst.
        let mut newest: Option<BotMsg> = None;
        loop {
            match rx.try_recv() {
                Ok(msg) => newest = Some(msg),
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    // Socket died: 10s of momentum, then forfeit (PLAN §4.3.4).
                    if !disconnected[b] {
                        disconnected[b] = true;
                        engine.disconnect(b as u32, engine.state.tick);
                    }
                    break;
                }
            }
        }
        if let Some(msg) = newest {
            let latency = msg.arrived.duration_since(tick_start).as_millis() as u64;
            engine.submit(b as u32, msg.input.clone(), latency);
            if msg.input.intent.is_some() || msg.input.belief.is_some() {
                engine.submit_mind(
                    b as u32,
                    msg.input.intent.clone(),
                    msg.input.belief.clone(),
                );
            }
            recorder.record_submit(b as u32, msg.input);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use tokio::sync::Mutex;
    use std::sync::Arc;

    #[test]
    fn seeds_come_from_os_entropy() {
        let a = random_seed();
        let b = random_seed();
        assert_ne!(a, b, "two OS-random u64s colliding is ~2^-64");
        assert_eq!(a & 1, 1, "seed stays odd (match-start invariant)");
        assert_eq!(b & 1, 1);
    }

    #[test]
    fn stalled_reader_is_disconnected_without_blocking_the_lane() {
        let mk = |name: &str, out_tx: mpsc::Sender<String>, in_rx: mpsc::Receiver<BotMsg>| {
            MatchEntrant {
                name: name.into(),
                db_id: 0,
                decision_rate: 1,
                auto_heel: false,
                connected: Arc::new(AtomicBool::new(true)),
                out_tx,
                in_rx: Arc::new(Mutex::new(in_rx)),
            }
        };

        let (tx_ok, mut rx_ok) = mpsc::channel::<String>(64);
        let (tx_stall, _rx_stall) = mpsc::channel::<String>(64); // never drained
        let (_in_tx_a, in_rx_a) = mpsc::channel::<BotMsg>(8);
        let (_in_tx_b, in_rx_b) = mpsc::channel::<BotMsg>(8);
        let entrants = vec![mk("ok", tx_ok, in_rx_a), mk("stalled", tx_stall, in_rx_b)];

        let mut engine =
            MatchEngine::new(MatchConfig::standard(), 1, &["ok".into(), "stalled".into()]);
        let mut stalls = vec![0u32; 2];

        // Fill the stalled bot's 64-slot channel, then keep pushing: the
        // STALL_LIMIT-th consecutive drop must move it onto the disconnect
        // path while the healthy bot keeps receiving every observation.
        for _ in 0..(64 + STALL_LIMIT) {
            while rx_ok.try_recv().is_ok() {}
            push_observations(&mut engine, &entrants, &mut stalls);
        }
        assert!(
            engine.timeouts[1].stats.disconnected_since_tick.is_some(),
            "stalled reader must enter the disconnect path"
        );
        assert!(
            engine.timeouts[0].stats.disconnected_since_tick.is_none(),
            "healthy bot must be untouched"
        );

        // And the healthy channel still works afterwards.
        while rx_ok.try_recv().is_ok() {}
        push_observations(&mut engine, &entrants, &mut stalls);
        assert!(
            rx_ok.try_recv().is_ok(),
            "healthy bot still receives observations"
        );
    }
}
