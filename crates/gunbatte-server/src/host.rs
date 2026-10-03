//! The in-process binding between the two roles: the matchmaker hands a
//! drafted roster to the game server and waits for the match to finish.
//! When the roles split across processes, this is the seam that becomes the
//! assignment protocol — until then the lobby learns nothing about how
//! matches run beyond `MatchContext` (gunbatte-node) and this signature.

use gunbatte_core::config::MatchConfig;
use gunbatte_gameserver::run_match;
use gunbatte_lobby::MatchHost;
use gunbatte_node::{MatchContext, MatchEntrant};
use futures_util::future::BoxFuture;

/// Runs each match in-process via the game-server role. Carries the
/// box-level knobs the game role honors but the matchmaker never sees.
pub struct GameHost {
    /// Reply-stamp acceptance window in ticks (see `accepts_tick`):
    /// 3 ≈ 350ms of tolerance for long-haul humans; 0 restores the strict
    /// #53 gate (only the exact tick being decided is accepted).
    input_window_ticks: u32,
}

impl GameHost {
    pub fn new(input_window_ticks: u32) -> Self {
        Self {
            input_window_ticks,
        }
    }
}

impl MatchHost for GameHost {
    fn host_match(
        &self,
        ctx: MatchContext,
        entrants: Vec<MatchEntrant>,
        config: MatchConfig,
    ) -> BoxFuture<'static, ()> {
        let input_window_ticks = self.input_window_ticks;
        Box::pin(async move {
            run_match(ctx, entrants, config, input_window_ticks).await;
        })
    }
}
