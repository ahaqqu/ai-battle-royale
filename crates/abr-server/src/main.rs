//! abr-server CLI: the one binary that runs the whole box (PLAN §8.1).

use abr_server::ServerConfig;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "abr-server", about = "GUNBATTE ROYALE ladder server")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the ladder server: bot gateway + queue + spectate + ladder page.
    Serve {
        #[arg(long, default_value_t = 8321)]
        port: u16,
        /// Address to listen on; use 127.0.0.1 behind a same-host reverse proxy.
        #[arg(long, default_value = "0.0.0.0")]
        bind: String,
        #[arg(long, default_value = "ladder.db")]
        db: PathBuf,
        #[arg(long, default_value = "replays")]
        replays: PathBuf,
        /// Built viewer directory to serve at /viewer/.
        #[arg(long, default_value = "viewer/dist")]
        viewer: PathBuf,
        /// Concurrent match lanes.
        #[arg(long, default_value_t = 2)]
        lanes: usize,
        /// Minimum connected bots to draft a match.
        #[arg(long, default_value_t = 2)]
        min_bots: usize,
        /// Max house bots used to top up a match when a human is queued
        /// (solo play); 0 disables.
        #[arg(long, default_value_t = 8)]
        house_bots: usize,
        /// Live spectate delay in seconds (anti-cheat; PLAN §6.2).
        #[arg(long, default_value_t = 0)]
        spectate_delay_s: u64,
        /// Seconds between keepalive pings to bot sockets (0 disables).
        #[arg(long, default_value_t = 10)]
        ws_ping_every_s: u64,
        /// Seconds of total silence before a bot socket is closed (0 disables).
        #[arg(long, default_value_t = 45)]
        ws_idle_timeout_s: u64,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Serve {
            port,
            bind,
            db,
            replays,
            viewer,
            lanes,
            min_bots,
            house_bots,
            spectate_delay_s,
            ws_ping_every_s,
            ws_idle_timeout_s,
        } => {
            let cfg = ServerConfig {
                port,
                bind,
                db_path: db,
                replay_dir: replays,
                viewer_dir: Some(viewer),
                lanes,
                min_bots,
                house_bots,
                spectate_delay_s,
                ws_ping_every_s,
                ws_idle_timeout_s,
            };
            abr_server::Server::start(cfg, abr_core::config::MatchConfig::standard())
                .await
                .expect("server");
        }
    }
}
