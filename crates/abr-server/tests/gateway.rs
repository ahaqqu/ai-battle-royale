//! M3 acceptance (PLAN §9): bots connect over WebSocket, get drafted into a
//! match, play through the 10Hz gateway loop with momentum on misses, and
//! the finished match writes a replay + updates the ladder + ELO.

use abr_core::config::MatchConfig;
use abr_server::{Server, ServerConfig};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

/// Drive one bot socket until the match ends; returns its match_over payload.
async fn play_as_bot(
    url: &str,
    name: &str,
    // given the observation JSON, produce the action JSON
    act: fn(&serde_json::Value) -> Option<serde_json::Value>,
    max_wait: Duration,
) -> serde_json::Value {
    let (ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("bot connects");
    let (mut tx, mut rx) = ws.split();
    tx.send(Message::Text(
        json!({"type": "register", "name": name, "decision_rate": 1}).to_string(),
    ))
    .await
    .unwrap();

    let deadline = tokio::time::Instant::now() + max_wait;
    while tokio::time::Instant::now() < deadline {
        let msg = tokio::time::timeout_at(deadline, rx.next()).await;
        let msg = match msg {
            Ok(Some(Ok(m))) => m,
            _ => break,
        };
        let Message::Text(text) = msg else { continue };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(json!(null));
        match v["type"].as_str() {
            Some("match_over") => return v,
            Some("registered") | Some("match_start") | Some("error") => continue,
            _ => {
                // Observation: reply with the scripted action.
                if let Some(action) = act(&v) {
                    tx.send(Message::Text(action.to_string())).await.ok();
                }
            }
        }
    }
    panic!("bot {name} never saw match_over");
}

fn idle_action(_obs: &serde_json::Value) -> Option<serde_json::Value> {
    Some(json!({"tick": _obs["tick"], "main": {"move": {"dir": 0, "throttle": 0.0}}}))
}

fn active_action(obs: &serde_json::Value) -> Option<serde_json::Value> {
    // March north and shield occasionally — legal but not smart.
    Some(json!({
        "tick": obs["tick"],
        "main": {"move": {"dir": 0, "throttle": 1.0}},
        "companion": {"move": {"dir": 180, "throttle": 1.0}}
    }))
}

#[tokio::test(flavor = "multi_thread")]
async fn m3_gateway_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let replay_dir = dir.path().join("replays");
    let port = 8931; // test-local port
    let cfg = ServerConfig {
        port,
        db_path: dir.path().join("ladder.db"),
        replay_dir: replay_dir.clone(),
        viewer_dir: None,
        lanes: 1,
        min_bots: 2,
        house_bots: 0,
        spectate_delay_s: 0,
    };
    tokio::spawn(async move {
        Server::start(cfg, MatchConfig::standard())
            .await
            .expect("server");
    });
    // Give the server a moment to bind.
    tokio::time::sleep(Duration::from_millis(600)).await;

    let url = format!("ws://127.0.0.1:{port}/ws/bot");
    let (a, b) = tokio::join!(
        play_as_bot(&url, "test-alpha", active_action, Duration::from_secs(240)),
        play_as_bot(&url, "test-beta", idle_action, Duration::from_secs(240)),
    );

    // Match must have completed and reported placements + a replay link.
    assert!(a["place"].as_i64().is_some(), "alpha got {a}");
    assert!(b["place"].as_i64().is_some(), "beta got {b}");
    assert_ne!(a["place"], b["place"], "placements must be distinct");
    let replay_url = a["replay"].as_str().expect("replay url");
    let replay_path = replay_dir.join(replay_url.trim_start_matches("/replays/"));
    assert!(
        replay_path.exists(),
        "replay file written: {}",
        replay_path.display()
    );

    // Replay must verify byte-identically.
    let bytes = std::fs::read(&replay_path).unwrap();
    let replay: abr_core::replay::Replay = serde_json::from_slice(&bytes).unwrap();
    abr_core::replay::verify_replay(&replay).expect("gateway replay re-simulates byte-identically");

    // Ladder must update: standings + matches list via the HTTP API.
    let http = format!("http://127.0.0.1:{port}");
    let body = reqwest_get(&format!("{http}/api/standings")).await;
    assert!(body.contains("test-alpha"), "standings: {body}");
    assert!(body.contains("test-beta"), "standings: {body}");
    let body = reqwest_get(&format!("{http}/api/matches")).await;
    assert!(
        body.contains(replay_url.trim_start_matches("/replays/")),
        "matches: {body}"
    );

    // The ladder page itself renders (at /ladder; / is the viewer).
    let body = reqwest_get(&format!("{http}/ladder")).await;
    assert!(body.contains("GUNBATTE<span>★</span>ROYALE"));
    assert!(body.contains("test-alpha"));
}

async fn reqwest_get(url: &str) -> String {
    // Minimal HTTP GET via raw TCP (avoid another dependency).
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (host, path) = {
        let rest = url.split_once("//").unwrap().1;
        match rest.split_once('/') {
            Some((h, p)) => (h.to_string(), format!("/{p}")),
            None => (rest.to_string(), "/".to_string()),
        }
    };
    let mut stream = tokio::net::TcpStream::connect(&host).await.unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.unwrap();
    String::from_utf8_lossy(&buf).to_string()
}

/// M4.5 acceptance (solo play): a single human registering `human: true` is
/// topped up to a full 8-entrant match with house bots — no other connected
/// bots, no waiting. House bots fight, the match completes, a replay is
/// written, and the human lands on the ladder while house bots stay off it.
#[tokio::test(flavor = "multi_thread")]
async fn solo_human_gets_house_fill() {
    let dir = tempfile::tempdir().unwrap();
    let replay_dir = dir.path().join("replays");
    let port = 8933;
    let cfg = ServerConfig {
        port,
        db_path: dir.path().join("ladder.db"),
        replay_dir: replay_dir.clone(),
        viewer_dir: None,
        lanes: 1,
        min_bots: 2,
        house_bots: 8,
        spectate_delay_s: 0,
    };
    // Short match cap so the test doesn't run a full 5-minute BR.
    let mut match_cfg = MatchConfig::standard();
    match_cfg.match_max_s = 25;
    tokio::spawn(async move {
        Server::start(cfg, match_cfg).await.expect("server");
    });
    tokio::time::sleep(Duration::from_millis(600)).await;

    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws/bot"))
        .await
        .expect("human connects");
    let (mut tx, mut rx) = ws.split();
    tx.send(Message::Text(
        json!({"type": "register", "name": "solo-human", "decision_rate": 1, "human": true})
            .to_string(),
    ))
    .await
    .unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    let mut entrants: Vec<String> = vec![];
    let mut over: Option<serde_json::Value> = None;
    while tokio::time::Instant::now() < deadline {
        let msg = tokio::time::timeout_at(deadline, rx.next()).await;
        let msg = match msg {
            Ok(Some(Ok(m))) => m,
            _ => break,
        };
        let Message::Text(text) = msg else { continue };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(json!(null));
        match v["type"].as_str() {
            Some("match_start") => {
                entrants = v["bots"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|b| b.as_str().map(String::from)).collect())
                    .unwrap_or_default();
            }
            Some("match_over") => {
                over = Some(v);
                break;
            }
            Some("registered") | Some("error") => {}
            _ => {
                // Observation: keep moving so the match stays live.
                tx.send(Message::Text(active_action(&v).unwrap().to_string())).await.ok();
            }
        }
    }
    let over = over.expect("human saw match_over within 90s");

    // The roster was topped up to 8: the human + 7 house bots.
    assert_eq!(entrants.len(), 8, "entrants: {entrants:?}");
    let house = entrants.iter().filter(|n| n.starts_with("house·")).count();
    assert_eq!(house, 7, "entrants: {entrants:?}");
    assert!(entrants.contains(&"solo-human".to_string()));

    // Match completed with a placement and a verifiable replay.
    assert!(over["place"].as_i64().is_some(), "over: {over}");
    let replay_url = over["replay"].as_str().expect("replay url");
    let replay_path = replay_dir.join(replay_url.trim_start_matches("/replays/"));
    assert!(replay_path.exists(), "replay written: {}", replay_path.display());
    let bytes = std::fs::read(&replay_path).unwrap();
    let replay: abr_core::replay::Replay = serde_json::from_slice(&bytes).unwrap();
    abr_core::replay::verify_replay(&replay).expect("house-fill replay verifies byte-identically");

    // Ladder shows the human; house bots stay off the standings.
    let body = reqwest_get(&format!("http://127.0.0.1:{port}/api/standings")).await;
    assert!(body.contains("solo-human"), "standings: {body}");
    assert!(!body.contains("house·"), "house bots must not appear: {body}");
}
