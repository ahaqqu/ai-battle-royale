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

/// Runs each match in-process via the game-server role.
pub struct GameHost;

impl MatchHost for GameHost {
    fn host_match(
        &self,
        ctx: MatchContext,
        entrants: Vec<MatchEntrant>,
        config: MatchConfig,
    ) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            run_match(ctx, entrants, config).await;
        })
    }
}
