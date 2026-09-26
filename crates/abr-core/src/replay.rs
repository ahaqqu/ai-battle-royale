//! Replay format (PLAN §5.3): map id + match config + seed + the ordered
//! per-tick input log (what each bot actually sent, including misses — the
//! engine's momentum logic is part of the deterministic re-simulation).
//! Per-tick state digests make byte-identical verification cheap.
//!
//! Contract: inputs recorded for tick T are submitted during tick T's window
//! and consumed by `step_tick()` number T. Verification replays exactly
//! that: submit the recorded inputs, then step, then compare digests.

use crate::config::MatchConfig;
use crate::engine::MatchEngine;
use crate::types::BotInput;
use serde::{Deserialize, Serialize};

pub const REPLAY_APIVERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReplayHeader {
    pub apiversion: u32,
    pub map_id: String,
    pub seed: u64,
    pub config: MatchConfig,
    pub bot_names: Vec<String>,
    pub decision_rates: Vec<u64>,
    pub auto_heel: Vec<bool>,
    pub finished: bool,
    pub winner: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReplayTick {
    /// One entry per bot: what arrived that tick (None = miss/none).
    pub inputs: Vec<Option<BotInput>>,
    pub digest: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Replay {
    pub header: ReplayHeader,
    pub ticks: Vec<ReplayTick>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReplaySummary {
    pub placements: Vec<Placed>,
    pub ticks: u64,
    pub winner: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Placed {
    pub bot: u32,
    pub name: String,
    pub kills: u32,
    pub hp_left: f64,
}

/// Captures the input log + digests while a match runs.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReplayRecorder {
    pub header: ReplayHeader,
    pub ticks: Vec<ReplayTick>,
    cur_inputs: Vec<Option<BotInput>>,
}

impl ReplayRecorder {
    pub fn new(engine: &MatchEngine, bot_names: &[String]) -> Self {
        ReplayRecorder {
            header: ReplayHeader {
                apiversion: REPLAY_APIVERSION,
                map_id: engine.config.map_id.clone(),
                seed: engine.seed,
                config: engine.config.clone(),
                bot_names: bot_names.to_vec(),
                decision_rates: engine.decision_rate.clone(),
                auto_heel: engine.auto_heel.clone(),
                finished: false,
                winner: None,
            },
            ticks: Vec::new(),
            cur_inputs: vec![None; bot_names.len()],
        }
    }

    pub fn record_submit(&mut self, bot: u32, input: BotInput) {
        self.cur_inputs[bot as usize] = Some(input);
    }

    pub fn record_tick(&mut self, digest: u64) {
        let n = self.header.bot_names.len();
        self.ticks.push(ReplayTick {
            inputs: std::mem::replace(&mut self.cur_inputs, vec![None; n]),
            digest,
        });
    }

    pub fn finish(&mut self, engine: &MatchEngine) {
        self.header.finished = engine.state.finished;
        self.header.winner = engine.state.winner;
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("replay serializes")
    }
}

/// Deterministic re-simulation of a replay (PLAN §9 M1 acceptance):
/// re-running the logged inputs must reproduce every tick's state digest
/// exactly. Returns Err at the first mismatch.
pub fn verify_replay(replay: &Replay) -> Result<ReplaySummary, String> {
    let bots = replay.header.bot_names.len() as u32;
    let mut engine = MatchEngine::new(
        replay.header.config.clone(),
        replay.header.seed,
        &replay.header.bot_names,
    );
    for (b, rate) in replay.header.decision_rates.iter().enumerate() {
        engine.decision_rate[b] = *rate;
    }
    engine.auto_heel = replay.header.auto_heel.clone();
    if engine.state.bots != bots {
        return Err("bot count mismatch".into());
    }
    for (ti, rt) in replay.ticks.iter().enumerate() {
        for (b, inp) in rt.inputs.iter().enumerate() {
            if let Some(inp) = inp {
                engine.submit(b as u32, inp.clone(), 0);
            }
        }
        engine.step_tick();
        let digest = engine.state.digest();
        if digest != rt.digest {
            return Err(format!(
                "tick {}: digest mismatch (recorded {:016x}, re-simulated {:016x})",
                ti + 1,
                rt.digest,
                digest
            ));
        }
    }
    Ok(build_summary(&engine, &replay.header.bot_names))
}

pub fn build_summary(engine: &MatchEngine, bot_names: &[String]) -> ReplaySummary {
    let st = &engine.state;
    let mut placements = st.placements.clone();
    if placements.is_empty() {
        // Never finished: rank by current placement then bot id.
        let mut order: Vec<(u32, Option<u32>)> =
            (0..st.bots).map(|b| (b, st.main(b).placement)).collect();
        order.sort_by_key(|(b, p)| (*p, *b));
        placements = order.into_iter().map(|(b, _)| b).collect();
    }
    let placements = placements
        .into_iter()
        .map(|b| {
            let m = st.main(b);
            Placed {
                bot: b,
                name: bot_names.get(b as usize).cloned().unwrap_or_default(),
                kills: m.kills,
                hp_left: crate::fixed::to_f64(m.hp),
            }
        })
        .collect();
    ReplaySummary {
        placements,
        ticks: st.tick,
        winner: st.winner,
    }
}
