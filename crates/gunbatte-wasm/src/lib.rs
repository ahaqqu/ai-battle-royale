//! WASM bindings over the deterministic core (PLAN §5.2, §7.1): the SAME
//! `step()`/`observe()` code the server runs re-simulates replays in the
//! browser, so replays stay thin and views stay exact.
//!
//! The viewer drives a `ReplaySim`: step() returns one spectator frame JSON
//! per tick; `observe_current(bot)` yields a strict-fog observation for
//! player-cam rendering.

use gunbatte_core::engine::MatchEngine;
use gunbatte_core::replay::Replay;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct ReplaySim {
    engine: MatchEngine,
    ticks: Vec<gunbatte_core::replay::ReplayTick>,
    cursor: usize, // ticks consumed so far
    bot_names: Vec<String>,
    decision_rates: Vec<u64>,
    auto_heel: Vec<bool>,
}

#[wasm_bindgen]
impl ReplaySim {
    #[wasm_bindgen(constructor)]
    pub fn new(replay_json: &str) -> Result<ReplaySim, JsValue> {
        let replay: Replay = serde_json::from_str(replay_json)
            .map_err(|e| JsValue::from_str(&format!("bad replay: {e}")))?;
        let bot_names = replay.header.bot_names.clone();
        let decision_rates = replay.header.decision_rates.clone();
        let auto_heel = replay.header.auto_heel.clone();
        let mut engine = MatchEngine::new(
            replay.header.config.clone(),
            replay.header.seed,
            &replay.header.bot_names,
        );
        engine.decision_rate = decision_rates.clone();
        engine.auto_heel = auto_heel.clone();
        let ticks = replay.ticks;
        Ok(ReplaySim {
            engine,
            ticks,
            cursor: 0,
            bot_names,
            decision_rates,
            auto_heel,
        })
    }

    /// Advance one tick, returning the full-state spectator frame as JSON.
    /// Returns null once the replay is exhausted.
    pub fn step(&mut self) -> Option<String> {
        if self.cursor >= self.ticks.len() {
            return None;
        }
        let rt = &self.ticks[self.cursor];
        for (b, inp) in rt.inputs.iter().enumerate() {
            if let Some(inp) = inp {
                if inp.intent.is_some() || inp.belief.is_some() {
                    // Mind-cam debug channel rides the recorded inputs.
                    self.engine
                        .submit_mind(b as u32, inp.intent.clone(), inp.belief.clone());
                }
                self.engine.submit(b as u32, inp.clone(), 0);
            }
        }
        let events = self.engine.step_tick();
        self.cursor += 1;
        Some(
            serde_json::to_string(&self.engine.spectator_frame(&events)).expect("frame serializes"),
        )
    }

    /// Strict-fog observation for one bot at the CURRENT tick (player-cam).
    pub fn observe_current(&self, bot: u32) -> Option<String> {
        if bot >= self.engine.state.bots {
            return None;
        }
        Some(serde_json::to_string(&self.engine.observe(bot)).expect("obs serializes"))
    }

    /// Jump back to tick 0 (e.g. before a player-cam re-simulation pass).
    pub fn reset(&mut self) {
        let seed = self.engine.seed;
        let cfg = self.engine.config.clone();
        let mut engine = MatchEngine::new(cfg, seed, &self.bot_names);
        engine.decision_rate = self.decision_rates.clone();
        engine.auto_heel = self.auto_heel.clone();
        self.engine = engine;
        self.cursor = 0;
    }

    pub fn total_ticks(&self) -> usize {
        self.ticks.len()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn bots(&self) -> usize {
        self.engine.state.bots as usize
    }

    pub fn bot_name(&self, bot: u32) -> Option<String> {
        self.bot_names.get(bot as usize).cloned()
    }

    pub fn map_id(&self) -> String {
        self.engine.config.map_id.clone()
    }

    /// Static map geometry (public knowledge, PLAN §2.4) for rendering.
    pub fn map_json(&self) -> String {
        serde_json::to_string(&self.engine.map.to_wire()).expect("map serializes")
    }

    pub fn seed(&self) -> f64 {
        self.engine.seed as f64
    }
}
