//! abr-runner: local CLI (PLAN §5.2). Runs bot-vs-bot matches headless with
//! the reference bots, writes replays, and verifies replays byte-identically.

use abr_core::bots::{self, RefBot};
use abr_core::config::MatchConfig;
use abr_core::engine::MatchEngine;
use abr_core::events::Event;
use abr_core::map::load_map;
use abr_core::replay::{build_summary, Replay, ReplayRecorder};
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
        #[arg(long, default_value = "replay.json")]
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
                (_, Some(p)) if p == "default16" => abr_core::bots::default16(),
                _ => {
                    eprintln!("--bots <a,b,c> or --preset default16 required");
                    std::process::exit(2);
                }
            };
            run(names, seed, out, verbose, full_info_dump);
        }
        Cmd::Verify { path } => verify(&path),
        Cmd::Bots => {
            println!("available reference bots:");
            for n in bots::BOT_NAMES {
                println!("  {n}");
            }
        }
    }
}

fn run(names: Vec<String>, seed: u64, out: String, verbose: bool, full_info_dump: bool) {
    let config = MatchConfig::default();
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
    match abr_core::replay::verify_replay(&replay) {
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
