//! Graceful degradation on bot timeouts (PLAN §4.3): momentum repeats keep
//! matches watchable; graduated penalties retire chronically slow bots
//! without punishing a GC pause.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeoutStats {
    pub replies: u64,
    pub missed: u64,
    pub slow_replies: u64,
    pub fatal_replies: u64,
    pub forfeit: Option<&'static str>,
    pub disconnected_since_tick: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct TimeoutTracker {
    pub cfg: crate::config::TimeoutConfig,
    pub stats: TimeoutStats,
    pub decided_ticks: u64,
}

impl TimeoutTracker {
    pub fn new(cfg: crate::config::TimeoutConfig) -> Self {
        TimeoutTracker {
            cfg,
            stats: TimeoutStats {
                replies: 0,
                missed: 0,
                slow_replies: 0,
                fatal_replies: 0,
                forfeit: None,
                disconnected_since_tick: None,
            },
            decided_ticks: 0,
        }
    }

    /// Record one expected decision point. `replied_ms`: Some(latency) if a
    /// valid reply arrived, None if the deadline passed with nothing.
    pub fn record(&mut self, replied_ms: Option<u64>) {
        if self.stats.forfeit.is_some() {
            return;
        }
        self.decided_ticks += 1;
        match replied_ms {
            Some(ms) => {
                self.stats.replies += 1;
                if ms > self.cfg.fatal_ms {
                    self.stats.fatal_replies += 1;
                    self.stats.forfeit = Some("reply exceeded fatal deadline (1s)");
                } else if ms > self.cfg.slow_ms {
                    self.stats.slow_replies += 1;
                    if self.stats.slow_replies >= self.cfg.max_slow_count {
                        self.stats.forfeit = Some("chronically slow (too many >200ms replies)");
                    }
                }
            }
            None => {
                self.stats.missed += 1;
                if self.decided_ticks > 0
                    && self.stats.missed * 100 >= self.cfg.max_missed_pct * self.decided_ticks
                    && self.stats.missed >= 10
                {
                    self.stats.forfeit = Some("missing too many deadlines (>20%)");
                }
            }
        }
    }

    /// Connection dropped mid-match: 10s of momentum before forfeit.
    pub fn disconnect(&mut self, tick: u64) {
        if self.stats.disconnected_since_tick.is_none() {
            self.stats.disconnected_since_tick = Some(tick);
        }
    }

    pub fn reconnect(&mut self) {
        self.stats.disconnected_since_tick = None;
    }

    /// Call each tick while disconnected. Returns a forfeit reason when the
    /// grace window (10s = 100 ticks) expires.
    pub fn tick_disconnected(&mut self, tick: u64) -> Option<&'static str> {
        if let Some(since) = self.stats.disconnected_since_tick {
            if tick.saturating_sub(since) >= self.cfg.disconnect_grace_ticks {
                let reason = "connection lost (10s grace expired)";
                self.stats.forfeit = Some(reason);
                self.stats.disconnected_since_tick = None;
                return Some(reason);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TimeoutConfig;

    fn cfg() -> TimeoutConfig {
        TimeoutConfig {
            slow_ms: 200,
            fatal_ms: 1000,
            max_slow_count: 30,
            max_missed_pct: 20,
            disconnect_grace_ticks: 100,
        }
    }

    #[test]
    fn normal_gc_pause_never_forfeits() {
        let mut t = TimeoutTracker::new(cfg());
        for i in 0..10_000 {
            // Occasional 300ms hiccup — under the 30 count.
            if i % 1000 == 0 {
                t.record(Some(300));
            } else {
                t.record(Some(30));
            }
        }
        assert_eq!(t.stats.forfeit, None);
    }

    #[test]
    fn fatal_reply_forfeits_immediately() {
        let mut t = TimeoutTracker::new(cfg());
        t.record(Some(1500));
        assert!(t.stats.forfeit.is_some());
    }

    #[test]
    fn chronic_slowness_forfeits() {
        let mut t = TimeoutTracker::new(cfg());
        for _ in 0..30 {
            t.record(Some(250));
        }
        assert!(t.stats.forfeit.is_some());
    }

    #[test]
    fn missed_deadlines_forfeit_at_threshold() {
        let mut t = TimeoutTracker::new(cfg());
        // 10 misses out of 51 decided = ~20%.
        for i in 0..51 {
            if i < 10 {
                t.record(None);
            } else {
                t.record(Some(10));
            }
        }
        assert!(t.stats.forfeit.is_some());
    }

    #[test]
    fn early_misses_dont_forfeit() {
        let mut t = TimeoutTracker::new(cfg());
        for _ in 0..9 {
            t.record(None);
        }
        for _ in 0..200 {
            t.record(Some(10));
        }
        assert_eq!(t.stats.forfeit, None);
    }

    #[test]
    fn disconnect_grace_window() {
        let mut t = TimeoutTracker::new(cfg());
        assert_eq!(t.tick_disconnected(10), None);
        t.disconnect(10);
        assert_eq!(t.tick_disconnected(50), None);
        assert_eq!(
            t.tick_disconnected(110),
            Some("connection lost (10s grace expired)")
        );
    }
}
