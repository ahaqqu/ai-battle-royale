//! Reference WebSocket bot client: connects one of the reference brains to
//! the gateway exactly like a player-hosted bot would (PLAN §4.1). Also the
//! living example of the bot protocol, including the optional mind-cam debug
//! channel: a client-side BeliefTracker fuses observations into a 64×64
//! "where I think everyone is" heat map (PLAN §3.5, §6.3).

use gunbatte_core::bots::{self, RefBot};
use gunbatte_core::map::load_map;
use gunbatte_core::observe::Observation;
use gunbatte_core::types::BotInput;
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::collections::BTreeMap;
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
    /// Enroll this identity on the ladder (issue #42): the server issues a
    /// secret on first connect, saved to the token file and replayed
    /// automatically afterwards. Without it the bot plays casual —
    /// off-ladder, no secret to manage.
    #[arg(long)]
    rated: bool,
    /// Where the issued secret is persisted for `--rated` runs.
    #[arg(long)]
    token_file: Option<String>,
    /// Publish the belief heat map on the mind-cam channel.
    #[arg(long, default_value_t = true)]
    mindcam: bool,
}

/// Purely client-side belief state (PLAN §3.5): the server never sends
/// ghosts — remembering and aging them is the bot's job. This one keeps a
/// last-seen registry and renders it as a 64×64 heat map for the mind-cam.
struct BeliefTracker {
    // main id → (x, y, tick seen, confidence 0..1)
    ghosts: BTreeMap<u32, (f64, f64, u64, f64)>,
    tick: u64,
}

const GRID: usize = 64;
const ARENA: f64 = 3200.0;

impl BeliefTracker {
    fn new() -> Self {
        BeliefTracker {
            ghosts: BTreeMap::new(),
            tick: 0,
        }
    }

    fn update(&mut self, obs: &Observation) {
        self.tick = obs.tick;
        for p in &obs.seen.players {
            let conf = if p.detail == "full" { 1.0 } else { 0.45 };
            let entry = self
                .ghosts
                .entry(p.id)
                .or_insert((p.pos[0], p.pos[1], obs.tick, conf));
            entry.0 = p.pos[0];
            entry.1 = p.pos[1];
            entry.2 = obs.tick;
            entry.3 = conf;
        }
        // Ghost aging: positions we haven't re-confirmed decay.
        for (_, _, seen_tick, conf) in self.ghosts.values_mut() {
            let age = (obs.tick.saturating_sub(*seen_tick)) as f64 / 10.0;
            *conf = (1.0 - age / 25.0).clamp(0.0, 1.0) * (*conf).max(0.3);
        }
        self.ghosts
            .retain(|_, (_, _, t, c)| obs.tick.saturating_sub(*t) < 600 && *c > 0.02);
        let _ = self.tick;
    }

    /// 64×64 row-major bytes, north-west origin (viewer convention).
    fn heat_map(&self) -> Vec<u8> {
        let mut grid = vec![0u8; GRID * GRID];
        for (x, y, _, conf) in self.ghosts.values() {
            let cx = (x / ARENA * GRID as f64) as isize;
            let cy = (y / ARENA * GRID as f64) as isize;
            if !(0..GRID as isize).contains(&cx) || !(0..GRID as isize).contains(&cy) {
                continue;
            }
            // Soft 3×3 splat with falloff.
            for dy in -1..=1isize {
                for dx in -1..=1isize {
                    let (gx, gy) = (cx + dx, cy + dy);
                    if !(0..GRID as isize).contains(&gx) || !(0..GRID as isize).contains(&gy) {
                        continue;
                    }
                    let falloff = if dx == 0 && dy == 0 {
                        1.0
                    } else if dx.abs() + dy.abs() == 1 {
                        0.45
                    } else {
                        0.15
                    };
                    let v = (conf * falloff * 255.0) as u32;
                    let idx = gy as usize * GRID + gx as usize;
                    grid[idx] = grid[idx].max(v.min(255) as u8);
                }
            }
        }
        grid
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let (ws, _) = tokio_tungstenite::connect_async(&cli.url)
        .await
        .expect("connect");
    println!("connected to {}", cli.url);
    let (mut tx, mut rx) = ws.split();

    // Auto-heel only for brains that ignore their companion (PLAN §2.3).
    let uses_companion = bots::create(&cli.bot, 0)
        .map(|b| b.uses_companion())
        .unwrap_or(false);
    // Identity secret (issue #42): an explicit --token wins; a --rated run
    // replays the saved secret, or registers tokenless so the server issues
    // a fresh one (saved to the token file when the ack arrives).
    let token_file = cli
        .token_file
        .clone()
        .unwrap_or_else(|| format!("{}.token", cli.name));
    let token = if !cli.token.is_empty() {
        cli.token.clone()
    } else if cli.rated {
        std::fs::read_to_string(&token_file)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_default()
    } else {
        String::new()
    };
    let reg = json!({
        "type": "register",
        "name": cli.name,
        "token": token,
        "rated": cli.rated,
        "decision_rate": cli.decision_rate,
        "auto_heel": !uses_companion,
    });
    tx.send(Message::Text(reg.to_string()))
        .await
        .expect("register");

    let mut map = load_map("arena-1").expect("map");
    let mut brain: Option<Box<dyn RefBot>> = None;
    let mut last_sent_tick = 0u64;
    let mut belief = BeliefTracker::new();

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
                let tier = if v["rated"] == true { "ladder" } else { "casual" };
                println!(
                    "registered as {} ({tier})",
                    v["you"].as_str().unwrap_or("?")
                );
                if let Some(t) = v["token"].as_str() {
                    match std::fs::write(&token_file, t) {
                        Ok(_) => println!("identity secret saved to {token_file}"),
                        Err(e) => {
                            eprintln!("could not save identity secret to {token_file}: {e}")
                        }
                    }
                }
            }
            Some("match_start") => {
                let map_id = v["map_id"].as_str().unwrap_or("arena-1").to_string();
                map = load_map(&map_id).unwrap_or_else(|| load_map("arena-1").unwrap());
                brain = bots::create(&cli.bot, 0);
                // New match: the observation stream restarts at tick 1, so
                // the stale-tick filter must reset too.
                last_sent_tick = 0;
                belief = BeliefTracker::new();
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
                if v["error"] == "bad token" {
                    eprintln!(
                        "hint: enroll once with --rated (the secret is saved to {token_file}), \
                         or pass --token with your issued secret"
                    );
                }
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
                        belief.update(&obs);
                        let mut input: BotInput = br.act(&obs, &map);
                        if cli.mindcam {
                            input.belief = Some(belief.heat_map());
                        }
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
