//! The match engine: wraps the deterministic sim with per-bot input slots,
//! momentum (graceful degradation), the auto-heel companion AI, timeout
//! tracking, and observation slicing. Used identically by the local runner
//! (M1) and the networked gateway (M3).

use crate::config::MatchConfig;
use crate::events::Event;
use crate::map::GameMap;
use crate::observe::{self, Observation, SpectatorFrame};
use crate::params::{AIM_LIMIT, SimParams};
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
    /// Why and when each bot forfeited, surfaced by the game role's journal
    /// events (the engine has no names to log).
    forfeit_info: Vec<Option<(&'static str, u64)>>,
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
            forfeit_info: vec![None; bots as usize],
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
        // (PLAN §4.3: be permissive at the edges of legality). Both units
        // get the same treatment: throttle is a speed multiplier (a crafted
        // 4.0 companion throttle used to run 4×), aim is clamped to ±2^40.
        let mut input = input;
        for unit in [&mut input.main, &mut input.companion] {
            unit.r#move.throttle = unit.r#move.throttle.clamp(0, crate::fixed::ONE);
            if let Some(UnitAction::Fire { target }) = unit.action.as_mut() {
                target.x = target.x.clamp(-AIM_LIMIT, AIM_LIMIT);
                target.y = target.y.clamp(-AIM_LIMIT, AIM_LIMIT);
            }
        }
        if let Some(i) = input.intent.as_mut() {
            truncate_shout(i);
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
        if !self.forfeited[bot as usize] {
            // The journal line lives with the game role (it owns the names);
            // the engine only records what happened and when.
            self.forfeit_info[bot as usize] = Some((reason, self.state.tick));
        }
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
    }

    /// Why and when this bot forfeited, if it has. Reason strings come from
    /// the timeout ladder ("missed deadline ladder", "disconnect grace", …).
    pub fn forfeit_info(&self, bot: u32) -> Option<(&'static str, u64)> {
        self.forfeit_info[bot as usize]
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
                // Start-of-match grace: handshakes and reconnections never
                // count as missed deadlines (30 ticks ≈ 3s).
                if tick > 30 {
                    self.timeouts[b].record(None);
                }
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
            per_unit[b * 2 + 1] = Some(UnitInput {
                r#move: Default::default(),
                action: Some(UnitAction::Heel),
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
        let intent = intent.map(|mut i| {
            truncate_shout(&mut i);
            i
        });
        self.last_mind_tick[bot as usize] = self.state.tick;
        self.minds.insert(bot, (intent, belief));
    }

    pub fn deadline_ms(&self) -> u64 {
        self.config.deadline_ms
    }
}

/// Spectator shouts cap at 64 chars, cut on a char boundary — `String::truncate`
/// panics when the cut lands inside a multi-byte char, which made this line
/// remotely triggerable from any bot's input message.
fn truncate_shout(s: &mut String) {
    if s.len() > 64 {
        *s = s.chars().take(64).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MatchConfig;

    #[test]
    fn intent_truncates_on_char_boundary() {
        let mut engine = MatchEngine::new(MatchConfig::standard(), 7, &["a".into(), "b".into()]);
        // 63 ASCII bytes + one 4-byte emoji: byte 64 splits the char, which
        // panicked `String::truncate(64)` before the guard.
        let shout = format!("{}😀", "a".repeat(63));

        let input = BotInput {
            intent: Some(shout.clone()),
            ..BotInput::default()
        };
        engine.submit(0, input, 0);
        engine.step_tick(); // must not panic

        // The mind channel receives the raw text too (drain_inputs feeds it
        // the untruncated input); its rate limit needs 5 ticks between updates.
        for _ in 0..6 {
            engine.step_tick();
        }
        engine.submit_mind(0, Some(shout), None);
        let stored = engine
            .minds
            .get(&0)
            .and_then(|(intent, _)| intent.as_ref())
            .expect("mind stored after rate window");
        assert_eq!(stored.chars().count(), 64);
    }

    /// Issue #40: extreme bot-supplied Fire targets must not overflow the
    /// i64 delta at the shot site — the submit clamp plus the wide delta
    /// keep debug builds panic-free with a legal normalized direction.
    #[test]
    fn fire_target_extremes_stay_legal() {
        let mut engine = MatchEngine::new(MatchConfig::standard(), 7, &["a".into(), "b".into()]);
        let aim = |x: i64, y: i64| UnitInput {
            r#move: crate::types::MoveInput::stop(),
            action: Some(UnitAction::Fire {
                target: crate::types::Vec2::new(x, y),
            }),
        };
        // Both units fire at opposite extremes every tick for a while.
        for tick in 0..30 {
            if tick % 3 == 0 {
                engine.submit(
                    0,
                    BotInput {
                        main: aim(i64::MIN, i64::MAX),
                        companion: aim(i64::MAX, i64::MIN),
                        ..BotInput::default()
                    },
                    0,
                );
                engine.submit(
                    1,
                    BotInput {
                        main: aim(-1, 1),
                        ..BotInput::default()
                    },
                    0,
                );
            }
            engine.step_tick();
        }
        // The engine never panicked (this test is the debug-build guard) and
        // every firing unit keeps a normalized 0..=359 facing.
        for unit in engine.state.units.iter() {
            assert!(unit.facing <= 359, "facing normalized: {}", unit.facing);
        }
    }

    /// Issue #40 acceptance: replay verification in a debug build survives a
    /// crafted extreme target — re-simulation goes through submit, so the
    /// clamp applies identically and digests match.
    #[test]
    fn replay_with_extreme_target_verifies() {
        let mut engine = MatchEngine::new(MatchConfig::standard(), 42, &["a".into(), "b".into()]);
        let names = vec!["a".to_string(), "b".to_string()];
        let mut rec = crate::replay::ReplayRecorder::new(&engine, &names);
        let crafted = BotInput {
            main: UnitInput {
                r#move: crate::types::MoveInput::stop(),
                action: Some(UnitAction::Fire {
                    target: crate::types::Vec2::new(i64::MIN, i64::MAX),
                }),
            },
            ..BotInput::default()
        };
        for _ in 0..10 {
            // The recorder logs the raw input, exactly like the gateway does;
            // bot 1 idles (recorded misses below the grace window aren't
            // reproducible by re-simulation, which never calls submit_miss).
            rec.record_submit(0, crafted.clone());
            rec.record_submit(1, BotInput::default());
            engine.submit(0, crafted.clone(), 0);
            engine.submit(1, BotInput::default(), 0);
            engine.step_tick();
            rec.record_tick(engine.state.digest());
        }
        rec.finish(&engine);
        let replay = crate::replay::Replay {
            header: rec.header.clone(),
            ticks: rec.ticks.clone(),
        };
        crate::replay::verify_replay(&replay).expect("crafted replay re-verifies in debug");
    }

    /// PR review on #65: the companion's throttle is a direct speed
    /// multiplier too — it must clamp exactly like the main's.
    #[test]
    fn companion_throttle_is_clamped_like_mains() {
        let mut engine = MatchEngine::new(MatchConfig::standard(), 7, &["a".into(), "b".into()]);
        let crafted = BotInput {
            main: UnitInput {
                r#move: crate::types::MoveInput {
                    dir: 0,
                    throttle: 4 * crate::fixed::ONE,
                },
                action: None,
            },
            companion: UnitInput {
                r#move: crate::types::MoveInput {
                    dir: 180,
                    throttle: -crate::fixed::ONE, // below zero also clamps
                },
                action: None,
            },
            ..BotInput::default()
        };
        engine.submit(0, crafted, 0);
        let stored = engine.pending[0].as_ref().expect("input stored");
        assert_eq!(stored.main.r#move.throttle, crate::fixed::ONE);
        assert_eq!(stored.companion.r#move.throttle, 0);
        engine.step_tick(); // must not panic
    }

    /// The shot site's own guard must be load-bearing on its own, without
    /// submit's clamp in front of it: an extreme target injected straight
    /// into the pending slot (as any future caller of `step` could) must
    /// not panic the i128→i64 narrow or `atan2_deg`'s `abs()` in debug.
    #[test]
    fn shot_site_narrowing_holds_without_the_submit_clamp() {
        let mut engine = MatchEngine::new(MatchConfig::standard(), 7, &["a".into(), "b".into()]);
        let aim = |x: i64, y: i64| UnitInput {
            r#move: crate::types::MoveInput::stop(),
            action: Some(UnitAction::Fire {
                target: crate::types::Vec2::new(x, y),
            }),
        };
        // Both axes land exactly on the narrow's old clamp bounds — the
        // values that used to reach `atan2_deg` as i64::MIN and panic abs().
        // Six idle ticks between shots let the fire cooldown expire, so
        // every combo actually reaches the delta computation.
        for (x, y) in [
            (i64::MIN, i64::MIN),
            (i64::MAX, i64::MAX),
            (i64::MIN, i64::MAX),
            (i64::MAX, 0),
        ] {
            engine.pending[0] = Some(BotInput {
                main: aim(x, y),
                companion: aim(y, x),
                ..BotInput::default()
            });
            engine.step_tick(); // must not panic
            for _ in 0..6 {
                engine.step_tick();
            }
            for unit in engine.state.units.iter() {
                assert!(unit.facing <= 359, "facing normalized: {}", unit.facing);
            }
        }
    }
}
