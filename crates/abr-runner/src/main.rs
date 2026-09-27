//! abr-runner: local CLI (PLAN §5.2, §6.2). Runs bot-vs-bot matches headless
//! with the reference bots, writes + verifies replays, and serves the web
//! viewer + replay library.

use abr_core::bots::{self, RefBot};
use abr_core::config::MatchConfig;
use abr_core::engine::MatchEngine;
use abr_core::events::Event;
use abr_core::map::load_map;
use abr_core::replay::{build_summary, verify_replay, Replay, ReplayRecorder};
use clap::{Parser, Subcommand};
use std::time::Instant;

#[derive(Parser)]
#[command(name = "abr-runner", about = "AI Battle Royale local runner")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a match between reference bots, headless, and write a replay.
    Run {
        /// Comma-separated reference bot names (e.g. hunter,camper,...).
        #[arg(long, group = "lineup")]
        bots: Option<String>,
        /// The default 16-entrant lineup.
        #[arg(long, group = "lineup")]
        preset: Option<String>,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long, default_value = "replays/match.json")]
        out: String,
        /// Log kills and zone events as they happen.
        #[arg(long)]
        verbose: bool,
        /// Debug flag: dump the first bot's observation once (local only).
        #[arg(long)]
        full_info_dump: bool,
    },
    /// Re-simulate a replay and check it reproduces byte-identically.
    Verify { path: String },
    /// Serve the viewer + replay library over HTTP.
    Serve {
        #[arg(long, default_value_t = 8321)]
        port: u16,
        /// Built viewer directory (viewer/dist).
        #[arg(long, default_value = "viewer/dist")]
        viewer: String,
        /// Replay library directory.
        #[arg(long, default_value = "replays")]
        replays: String,
    },
    /// List the available reference bots.
    Bots,
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run {
            bots,
            preset,
            seed,
            out,
            verbose,
            full_info_dump,
        } => {
            let names = match (bots, preset) {
                (Some(b), _) => b.split(',').map(|s| s.trim().to_string()).collect(),
                (_, Some(p)) if p == "default8" => abr_core::bots::default8(),
                (_, Some(p)) if p == "default16" => abr_core::bots::default16(),
                _ => {
                    eprintln!("--bots <a,b,c> or --preset default8 required");
                    std::process::exit(2);
                }
            };
            run(names, seed, out, verbose, full_info_dump);
        }
        Cmd::Verify { path } => verify(&path),
        Cmd::Serve {
            port,
            viewer,
            replays,
        } => serve(port, &viewer, &replays),
        Cmd::Bots => {
            println!("available reference bots:");
            for n in bots::BOT_NAMES {
                println!("  {n}");
            }
        }
    }
}

fn run(names: Vec<String>, seed: u64, out: String, verbose: bool, full_info_dump: bool) {
    let config = MatchConfig::standard();
    let mut engine = MatchEngine::new(config, seed, &names);
    for (b, name) in names.iter().enumerate() {
        let uses_companion = bots::create(name, b as u32)
            .map(|bt| bt.uses_companion())
            .unwrap_or(false);
        engine.configure_bot(b as u32, 1, !uses_companion && engine.config.auto_heel);
    }
    let map = load_map(&engine.config.map_id).expect("map");
    let mut brains: Vec<Box<dyn RefBot>> = names
        .iter()
        .enumerate()
        .map(|(b, n)| bots::create(n, b as u32).unwrap_or_else(|| panic!("unknown bot {n}")))
        .collect();
    let mut recorder = ReplayRecorder::new(&engine, &names);

    let t0 = Instant::now();
    let mut kills_seen: usize = 0;
    let mut zone_events: usize = 0;
    let mut dumped = false;
    while !engine.state.finished {
        // Tick window: every bot gets its observation at the same moment and
        // replies inside the deadline (local bots reply instantly).
        for b in 0..names.len() as u32 {
            let obs = engine.observe(b);
            if full_info_dump && !dumped && b == 0 && engine.state.tick == 1 {
                eprintln!(
                    "sample observation[0]:\n{}",
                    serde_json::to_string_pretty(&obs).unwrap()
                );
                dumped = true;
            }
            let input = brains[b as usize].act(&obs, &map);
            engine.submit(b, input.clone(), 0);
            recorder.record_submit(b, input);
        }
        let events = engine.step_tick();
        for e in &events {
            match e {
                Event::Death { bot, killer, .. } => {
                    kills_seen += 1;
                    if verbose {
                        match killer {
                            Some(k) => println!("t{}: {k} eliminated {bot}", engine.state.tick),
                            None => println!("t{}: {bot} died to the zone", engine.state.tick),
                        }
                    }
                }
                Event::ZoneShrinkStarted { phase, .. } => {
                    zone_events += 1;
                    if verbose {
                        println!("t{}: zone phase {phase} shrinking", engine.state.tick);
                    }
                }
                Event::MatchEnded { winner } => {
                    println!("t{}: match over, winner: {:?}", engine.state.tick, winner);
                }
                _ => {}
            }
        }
        recorder.record_tick(engine.state.digest());
        if engine.state.tick > 10_000 {
            eprintln!("safety cap hit");
            break;
        }
    }
    let elapsed = t0.elapsed();
    recorder.finish(&engine);

    let summary = build_summary(&engine, &names);
    let json = recorder.to_json();
    if let Some(parent) = std::path::Path::new(&out).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&out, &json).expect("write replay");
    let bytes = json.len();

    println!("\n=== match summary ===");
    println!(
        "map: {} · seed: {seed} · bots: {}",
        engine.config.map_id,
        names.len()
    );
    println!(
        "ticks: {} ({:.1}s game time)",
        summary.ticks,
        summary.ticks as f64 / 10.0
    );
    println!(
        "engine wall time: {:.1}ms ({:.1}µs/tick)",
        elapsed.as_secs_f64() * 1000.0,
        elapsed.as_micros() as f64 / summary.ticks.max(1) as f64
    );
    println!("replay: {out} ({:.1} KB)", bytes as f64 / 1024.0);
    println!("kills: {kills_seen}, zone phases advanced: {zone_events}");
    println!("\nplacements:");
    for (rank, p) in summary.placements.iter().enumerate() {
        println!(
            "  {:>2}. {:<12} (bot {}) — {} kills, {:.0} hp left",
            rank + 1,
            p.name,
            p.bot,
            p.kills,
            p.hp_left
        );
    }
}

fn verify(path: &str) {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let replay: Replay =
        serde_json::from_slice(&data).unwrap_or_else(|e| panic!("parse replay: {e}"));
    let t0 = Instant::now();
    match verify_replay(&replay) {
        Ok(summary) => {
            println!(
                "OK: {} ticks re-simulated byte-identically in {:.0}ms; winner: bot {:?}",
                summary.ticks,
                t0.elapsed().as_millis(),
                summary.winner
            );
        }
        Err(e) => {
            eprintln!("FAIL: {e}");
            std::process::exit(1);
        }
    }
}

fn serve(port: u16, viewer_dir: &str, replays_dir: &str) {
    use axum::response::IntoResponse;
    use axum::routing::get;
    use tower_http::services::{ServeDir, ServeFile};

    let viewer_path = std::path::PathBuf::from(viewer_dir);
    let index = viewer_path.join("index.html");
    if !index.exists() {
        eprintln!("viewer build not found at {index:?}. Build it: (cd viewer && npm install && npm run build)");
        std::process::exit(1);
    }
    let replays = std::path::PathBuf::from(replays_dir);
    std::fs::create_dir_all(&replays).ok();
    let replays_for_list = replays.clone();

    let app = axum::Router::new()
        .route(
            "/api/replays",
            get(move || async move {
                let mut items = vec![];
                if let Ok(rd) = std::fs::read_dir(&replays_for_list) {
                    for entry in rd.flatten() {
                        let path = entry.path();
                        if path.extension().is_some_and(|e| e == "json") {
                            let name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                            items.push(serde_json::json!({
                                "name": name,
                                "url": format!("/replays/{name}"),
                                "size_kb": size as f64 / 1024.0,
                            }));
                        }
                    }
                }
                items.sort_by(|a, b| b["name"].as_str().cmp(&a["name"].as_str()));
                axum::Json(items).into_response()
            }),
        )
        .nest_service(
            "/replays",
            ServeDir::new(&replays).append_index_html_on_directories(false),
        )
        .fallback_service(ServeDir::new(&viewer_path).not_found_service(ServeFile::new(index)));

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    println!("▶ AI Battle Royale viewer: http://127.0.0.1:{port}");
    println!("  replay library: {replays_dir:?} → /replays/<file>.json");
    runtime
        .block_on(async move {
            let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
                .await
                .expect("bind");
            axum::serve(listener, app).await
        })
        .expect("server");
}
