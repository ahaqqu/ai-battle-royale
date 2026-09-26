//! Reference WebSocket bot client: connects one of the reference brains to
//! the gateway exactly like a player-hosted bot would (PLAN §4.1). Also the
//! living example of the bot protocol.

use abr_core::bots::{self, RefBot};
use abr_core::config::MatchConfig;
use abr_core::map::load_map;
use abr_core::observe::Observation;
use abr_core::types::BotInput;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

#[derive(Parser)]
struct Cli {
    /// Gateway URL, e.g. ws://127.0.0.1:8321/ws/bot
    url: String,
    #[arg(long)]
    name: String,
    /// Reference brain to run (hunter, camper, looter, survivor, berserker, wanderer).
    #[arg(long, default_value = "hunter")]
    bot: String,
    /// Act every Nth tick (PLAN §4.2 slow decision rate).
    #[arg(long, default_value_t = 1)]
    decision_rate: u64,
    #[arg(long, default_value = "")]
    token: String,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let (ws, _) = tokio_tungstenite::connect_async(&cli.url)
        .await
        .expect("connect");
    println!("connected to {}", cli.url);
    let (mut tx, mut rx) = ws.split();

    let reg = json!({
        "type": "register",
        "name": cli.name,
        "token": cli.token,
        "decision_rate": cli.decision_rate,
    });
    tx.send(Message::Text(reg.to_string()))
        .await
        .expect("register");

    // Wait for registration + match start; remember the map.
    let mut map = load_map("arena-1").expect("map");
    let mut brain: Option<Box<dyn RefBot>> = None;
    let mut last_sent_tick = 0u64;

    while let Some(msg) = rx.next().await {
        let msg = match msg {
            Ok(Message::Text(t)) => t,
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(e) => {
                eprintln!("ws error: {e}");
                break;
            }
        };
        let v: serde_json::Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match v["type"].as_str() {
            Some("registered") => {
                println!("registered as {}", v["you"].as_str().unwrap_or("?"));
            }
            Some("match_start") => {
                let map_id = v["map_id"].as_str().unwrap_or("arena-1").to_string();
                map = load_map(&map_id).unwrap_or_else(|| load_map("arena-1").unwrap());
                brain = bots::create(&cli.bot, 0);
                // New match: the observation stream restarts at tick 1, so
                // the stale-tick filter must reset too.
                last_sent_tick = 0;
                println!("match started on {map_id} — playing {}", cli.bot);
            }
            Some("match_over") => {
                println!(
                    "match over — place {} — replay {}",
                    v["place"],
                    v["replay"].as_str().unwrap_or("?")
                );
                brain = None;
            }
            Some("error") => {
                eprintln!("server error: {}", v["error"]);
                break;
            }
            None | Some(_) => {
                // No "type" field → an Observation for the current tick.
                if let Ok(obs) = serde_json::from_str::<Observation>(&msg) {
                    if obs.tick <= last_sent_tick {
                        continue; // stale duplicate
                    }
                    last_sent_tick = obs.tick;
                    if let Some(br) = brain.as_mut() {
                        let input: BotInput = br.act(&obs, &map);
                        let mut out = serde_json::to_value(&input).unwrap_or_default();
                        out["tick"] = json!(obs.tick);
                        if tx.send(Message::Text(out.to_string())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    }
    println!("disconnected");
}
