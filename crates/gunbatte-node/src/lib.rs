//! Node seam: the narrow contract every role of a deployment node shares —
//! the entrant handed from matchmaking into a match, the per-match resources
//! the game role receives, and the ladder database that is the only
//! cross-role state (AGENTS.md: matchmaker ≠ game server). Matchmaking
//! assembles entrants; the game role consumes the entrant view and writes
//! results back through the database. Neither role depends on the other.

pub mod db;

use gunbatte_core::types::BotInput;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, mpsc, Mutex};

/// One entrant of a match, as the game role sees it. This is the whole
/// handoff: matchmaking owns everything else about a connection (queue and
/// lobby membership, mode, human flag); the match loop consumes an entrant
/// and never looks back.
#[derive(Clone)]
pub struct MatchEntrant {
    pub name: String,
    /// Ladder row id (0 when the row was never created).
    pub db_id: i64,
    pub decision_rate: u64,
    pub auto_heel: bool,
    /// Liveness flag shared with whoever owns the connection.
    pub connected: Arc<AtomicBool>,
    /// Server → entrant: observations and lifecycle events, as JSON text.
    pub out_tx: mpsc::Sender<String>,
    /// Entrant → server: at most one live message per tick window.
    pub in_rx: Arc<Mutex<mpsc::Receiver<BotMsg>>>,
}

/// One message from an entrant inside a tick's reply window.
pub struct BotMsg {
    /// Client-asserted tick — currently advisory; the loop coalesces to the
    /// newest message per window regardless.
    pub client_tick: u64,
    pub input: BotInput,
    pub arrived: Instant,
}

/// Per-match resources the matchmaker hands the game role: where replays
/// and results go, and where spectator frames are published. Everything
/// else a match needs arrives via `MatchConfig` and the entrants.
pub struct MatchContext {
    pub db: Arc<db::Db>,
    pub replay_dir: PathBuf,
    pub spectate: broadcast::Sender<String>,
}
