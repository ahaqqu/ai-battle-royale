//! The match engine: wraps the deterministic sim with per-bot input slots,
//! momentum (graceful degradation), the auto-heel companion AI, timeout
//! tracking, and observation slicing. Used identically by the local runner
//! (M1) and the networked gateway (M3).

use crate::config::MatchConfig;
use crate::events::Event;
use crate::map::GameMap;
use crate::observe::{self, Observation, SpectatorFrame};
use crate::params::SimParams;
use crate::state::WorldState;
use crate::timeout::TimeoutTracker;
use crate::types::{BotInput, UnitAction, UnitInput};
use std::collections::BTreeMap;

pub struct MatchEngine {
    pub state: WorldState,
    pub params: SimParams,
    pub map: GameMap,
    pub config: MatchConfig,
    /// Original seed — recorded into replays, never sent to bots (PLAN §3.3).
    pub seed: u64,
    /// Last accepted input per bot (momentum source).
    last_inputs: Vec<Option<BotInput>>,
    /// Input slot for the tick currently being assembled.
    pending: Vec<Option<BotInput>>,
    pub timeouts: Vec<TimeoutTracker>,
    /// Per-bot decision rate: act every Nth tick (1 = every tick).
    pub decision_rate: Vec<u64>,
    /// Bots that never send companion commands get the auto-heel AI.
    pub auto_heel: Vec<bool>,
    companion_cmds: Vec<u64>,
    /// Mind-cam debug channel (intent, belief), rate-limited by the engine.
    pub minds: BTreeMap<u32, (Option<String>, Option<Vec<u8>>)>,
    last_mind_tick: Vec<u64>,
    /// Bot forfeited (timeout ladder) — momentum until the end.
    forfeited: Vec<bool>,
}

impl MatchEngine {
    pub fn new(config: MatchConfig, seed: u64, bot_names: &[String]) -> Self {
        let params = SimParams::from_config(&config);
        let map = crate::map::load_map(&config.map_id)
            .unwrap_or_else(|| panic!("unknown map {}", config.map_id));
        let bots = bot_names.len() as u32;
        assert!(
            bots >= 2 && bots as usize <= map.spawns.len(),
            "2..=16 bots required"
        );
        let state = WorldState::new(&params, &map, bots, seed);
        MatchEngine {
            state,
            params,
            map,
            seed,
            config,
            last_inputs: vec![None; bots as usize],
            pending: vec![None; bots as usize],
            timeouts: (0..bots)
                .map(|_| TimeoutTracker::new(crate::config::TimeoutConfig::default()))
                .collect(),
            decision_rate: vec![1; bots as usize],
            auto_heel: vec![false; bots as usize],
            companion_cmds: vec![0; bots as usize],
            minds: BTreeMap::new(),
            last_mind_tick: vec![0; bots as usize],
            forfeited: vec![false; bots as usize],
        }
    }

    /// Configure decision rate + auto-heel per bot (called before tick 0).
    pub fn configure_bot(&mut self, bot: u32, decision_rate: u64, auto_heel: bool) {
        self.decision_rate[bot as usize] = decision_rate.clamp(1, 10);
        self.auto_heel[bot as usize] = auto_heel;
    }

    /// Bot replied. `latency_ms` feeds the timeout ladder (0 for local sims).
    pub fn submit(&mut self, bot: u32, input: BotInput, latency_ms: u64) {
        if self.state.finished {
            return;
        }
        // Validate: clamp what we can, keep it legal-but-bad otherwise
        // (PLAN §4.3: be permissive at the edges of legality).
        let mut input = input;
        input.main.r#move.throttle = input.main.r#move.throttle.clamp(0, crate::fixed::ONE);
        if let Some(i) = input.intent.as_mut() {
            i.truncate(64);
        }
        self.pending[bot as usize] = Some(input);
        self.timeouts[bot as usize].record(Some(latency_ms));
    }

    /// Bot missed its deadline — momentum keeps it playing (PLAN §4.3.1).
    pub fn submit_miss(&mut self, bot: u32) {
        if self.state.finished {
            return;
        }
        self.timeouts[bot as usize].record(None);
    }

    pub fn disconnect(&mut self, bot: u32, tick: u64) {
        self.timeouts[bot as usize].disconnect(tick);
    }

    pub fn reconnect(&mut self, bot: u32) {
        self.timeouts[bot as usize].reconnect();
    }

    pub fn forfeit(&mut self, bot: u32, reason: &'static str) {
        self.forfeited[bot as usize] = true;
        // A forfeited bot stops acting; its main is eliminated.
        let st = &mut self.state;
        if st.main(bot).alive {
            st.main_mut(bot).hp = 0;
            st.main_mut(bot).last_damager = None;
            st.main_mut(bot).last_damager_tick = 0;
        }
        self.pending[bot as usize] = None;
        self.last_inputs[bot as usize] = None;
        let _ = reason;
    }

    pub fn is_forfeited(&self, bot: u32) -> bool {
        self.forfeited[bot as usize]
    }

    /// Advance one tick. Applies momentum, auto-heel, runs the sim.
    pub fn step_tick(&mut self) -> Vec<Event> {
        if self.state.finished {
            return vec![];
        }
        let tick = self.state.tick + 1;

        // Timeout ladder bookkeeping: disconnected grace + auto-forfeit on
        // any recorded forfeit condition (deterministic in run and replay).
        let bots = self.state.bots;
        for b in 0..bots {
            if let Some(reason) = self.timeouts[b as usize].tick_disconnected(tick) {
                self.forfeit(b, reason);
            }
            if let Some(reason) = self.timeouts[b as usize].stats.forfeit {
                if !self.forfeited[b as usize] {
                    self.forfeit(b, reason);
                }
            }
        }

        // Build per-unit effective inputs: momentum fill + auto-heel.
        let mut per_unit: Vec<Option<UnitInput>> = vec![None; (bots * 2) as usize];
        for b in 0..bots as usize {
            if self.forfeited[b] {
                continue;
            }
            let on_decision_tick = tick
                .saturating_sub(1)
                .is_multiple_of(self.decision_rate[b].max(1));
            if !on_decision_tick {
                // Between decision ticks: last action repeats (§4.2).
            } else if self.pending[b].is_none() {
                self.timeouts[b].record(None);
            } else {
                self.last_inputs[b] = self.pending[b].take();
            }
            if let Some(inp) = &self.last_inputs[b] {
                per_unit[b * 2] = Some(inp.main);
                per_unit[b * 2 + 1] = Some(inp.companion);
            }
        }

        // Auto-heel: drive companions whose bot never sends companion cmds —
        // stay near the main, ping on cooldown (PLAN §2.3).
        for b in 0..bots as usize {
            if !self.auto_heel[b] || self.forfeited[b] {
                continue;
            }
            let comp = self.state.companion(b as u32);
            if !comp.alive {
                continue;
            }
            let sonar_ready = comp.sonar_cd <= 0 && comp.energy >= self.params.sonar_cost;
            let action = if sonar_ready {
                Some(UnitAction::Sonar)
            } else {
                Some(UnitAction::Heel)
            };
            per_unit[b * 2 + 1] = Some(UnitInput {
                r#move: Default::default(),
                action,
            });
        }

        // Run the deterministic sim.
        let events = crate::step::step(&mut self.state, &self.params, &self.map, &per_unit);

        // Track companion-command usage for auto-heel opt-out.
        for b in 0..bots as usize {
            if let Some(inp) = &self.last_inputs[b] {
                if inp.companion.action.is_some() || inp.companion.r#move.throttle != 0 {
                    self.companion_cmds[b] += 1;
                }
            }
        }

        events
    }

    /// Strict-fog observation for one bot (PLAN §3).
    pub fn observe(&self, bot: u32) -> Observation {
        observe::observe(&self.state, &self.params, &self.map, bot)
    }

    /// Full-state frame for spectators/replays (PLAN §6).
    pub fn spectator_frame(&self, events: &[Event]) -> SpectatorFrame {
        observe::spectator_frame(&self.state, &self.params, events, &self.minds)
    }

    /// Bot sent a mind-cam update (rate-limited to every 5 ticks, PLAN §6.3).
    pub fn submit_mind(&mut self, bot: u32, intent: Option<String>, belief: Option<Vec<u8>>) {
        if self
            .state
            .tick
            .saturating_sub(self.last_mind_tick[bot as usize])
            < 5
        {
            return;
        }
        if let Some(b) = &belief {
            if b.len() > 4096 {
                return;
            }
        }
        self.last_mind_tick[bot as usize] = self.state.tick;
        self.minds.insert(bot, (intent, belief));
    }

    pub fn deadline_ms(&self) -> u64 {
        self.config.deadline_ms
    }
}
