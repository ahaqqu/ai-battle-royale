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
        bind: "127.0.0.1".to_string(),
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
        bind: "127.0.0.1".to_string(),
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

/// Lobby (PLAN extension): a host creates a room, shares the code, an
/// invitee joins, the host starts by hand — and the match drafts exactly the
/// room's roster (topped up with house bots), never the public queue.
/// Works the same for royale and boss; this is the royale half.
#[tokio::test(flavor = "multi_thread")]
async fn lobby_host_and_invitee_play_a_private_royale() {
    let dir = tempfile::tempdir().unwrap();
    let replay_dir = dir.path().join("replays");
    let port = 8935;
    let cfg = ServerConfig {
        port,
        bind: "127.0.0.1".to_string(),
        db_path: dir.path().join("ladder.db"),
        replay_dir: replay_dir.clone(),
        viewer_dir: None,
        lanes: 1,
        min_bots: 2,
        house_bots: 8,
        spectate_delay_s: 0,
    };
    let mut match_cfg = MatchConfig::standard();
    match_cfg.match_max_s = 25;
    tokio::spawn(async move {
        Server::start(cfg, match_cfg).await.expect("server");
    });
    tokio::time::sleep(Duration::from_millis(600)).await;

    let url = format!("ws://127.0.0.1:{port}/ws/bot");
    let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let (mut host_tx, mut host_rx) = ws.split();
    host_tx
        .send(Message::Text(
            json!({"type":"register","name":"lobby-host","decision_rate":1,"human":true,
                   "lobby_action":"create"})
                .to_string(),
        ))
        .await
        .unwrap();

    // Host learns the share code.
    let code = loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), host_rx.next())
            .await
            .expect("host hears back")
            .unwrap()
            .unwrap();
        let Message::Text(t) = msg else { continue };
        let v: serde_json::Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "lobby_joined" {
            assert_eq!(v["host"], "lobby-host");
            assert_eq!(v["members"].as_array().unwrap().len(), 1);
            break v["lobby"].as_str().unwrap().to_string();
        }
    };
    assert_eq!(code.len(), 4, "share code looks like K7QP: {code}");

    // The public scheduler must not steal a lobby member: a human-flagged
    // solo queuer would normally be house-filled within one 2s pass, so if the
    // host were visible to the queue a match_start would land here.
    let quiet = tokio::time::Instant::now() + Duration::from_millis(4600);
    while tokio::time::Instant::now() < quiet {
        match tokio::time::timeout_at(quiet, host_rx.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                assert_ne!(
                    v["type"], "match_start",
                    "the public queue drafted a lobby member: {v}"
                );
            }
            Ok(Some(Ok(_))) => {}
            _ => break,
        }
    }
    // The invitee joins the room by code.
    let (ws2, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let (mut guest_tx, mut guest_rx) = ws2.split();
    guest_tx
        .send(Message::Text(
            json!({"type":"register","name":"lobby-guest","decision_rate":1,"human":true,
                   "lobby_action":"join","lobby":code})
                .to_string(),
        ))
        .await
        .unwrap();

    let mut host_started = false;
    let mut guest_started = false;
    let mut host_roster = 0usize;
    let mut started_sent = false;
    // Host sees the roster update, then starts the match by hand.
    let started_at = tokio::time::Instant::now();
    while started_at.elapsed() < Duration::from_secs(30) && !(host_started && guest_started) {
        tokio::select! {
            msg = host_rx.next() => {
                let Some(Ok(Message::Text(t))) = msg else { break };
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                match v["type"].as_str() {
                    Some("lobby_roster") => {
                        host_roster = v["members"].as_array().unwrap().len();
                        if host_roster >= 2 && !started_sent {
                            // The whole point of a lobby: the host presses start.
                            started_sent = true;
                            host_tx
                                .send(Message::Text(
                                    json!({"type":"lobby_start","action":"start"}).to_string(),
                                ))
                                .await
                                .unwrap();
                        }
                    }
                    Some("match_start") => {
                        host_started = true;
                        let entrants: Vec<String> = v["bots"].as_array().unwrap().iter()
                            .filter_map(|b| b.as_str().map(String::from)).collect();
                        assert!(entrants.contains(&"lobby-host".to_string()));
                        assert!(entrants.contains(&"lobby-guest".to_string()),
                            "the invitee plays: {entrants:?}");
                        assert_eq!(entrants.len(), 8, "house-filled to 8: {entrants:?}");
                        host_tx.send(Message::Text(active_action(&v).unwrap().to_string())).await.ok();
                    }
                    Some("error") => panic!("host got error: {v}"),
                    _ => { host_tx.send(Message::Text(active_action(&v).unwrap().to_string())).await.ok(); }
                }
            }
            msg = guest_rx.next() => {
                let Some(Ok(Message::Text(t))) = msg else { break };
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                if v["type"] == "lobby_joined" {
                    assert_eq!(v["lobby"], code);
                    assert_eq!(v["members"].as_array().unwrap().len(), 2);
                } else if v["type"] == "match_start" {
                    guest_started = true;
                }
                guest_tx.send(Message::Text(active_action(&v).unwrap_or(json!({"tick":0})).to_string())).await.ok();
            }
        }
    }
    assert!(host_started && guest_started, "both lobby members entered the match");
    assert_eq!(host_roster, 2, "host saw the invitee in the roster");

    // Manual start actually plays the match out: both get placements.
    let mut host_over: Option<serde_json::Value> = None;
    let mut guest_over: Option<serde_json::Value> = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    while tokio::time::Instant::now() < deadline && (host_over.is_none() || guest_over.is_none()) {
        tokio::select! {
            msg = host_rx.next() => {
                let Some(Ok(Message::Text(t))) = msg else { break };
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                if v["type"] == "match_over" { host_over = Some(v); }
                else if v["type"].is_null() { host_tx.send(Message::Text(active_action(&v).unwrap().to_string())).await.ok(); }
            }
            msg = guest_rx.next() => {
                let Some(Ok(Message::Text(t))) = msg else { break };
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                if v["type"] == "match_over" { guest_over = Some(v); }
                else if v["type"].is_null() { guest_tx.send(Message::Text(active_action(&v).unwrap().to_string())).await.ok(); }
            }
        }
    }
    let host_over = host_over.expect("host saw match_over");
    assert!(host_over["place"].as_i64().is_some(), "host: {host_over}");
    let guest_over = guest_over.expect("guest saw match_over");
    assert!(guest_over["place"].as_i64().is_some(), "guest: {guest_over}");

    // The room's match is a real, verifiable replay with mode royale.
    let replay_url = host_over["replay"].as_str().expect("replay url");
    let replay_path = replay_dir.join(replay_url.trim_start_matches("/replays/"));
    let replay: abr_core::replay::Replay =
        serde_json::from_slice(&std::fs::read(&replay_path).unwrap()).unwrap();
    assert_eq!(replay.header.config.mode, abr_core::config::GameMode::Royale);
    abr_core::replay::verify_replay(&replay).expect("lobby replay verifies byte-identically");
}

/// Lobby + Slain the Boss: a boss-lobby casts one member (the host's pick) as
/// the arena boss, fills the raider slots with house bots, and the raid runs
/// to a real, verifiable finish.
#[tokio::test(flavor = "multi_thread")]
async fn boss_lobby_casts_a_member_as_the_boss() {
    let dir = tempfile::tempdir().unwrap();
    let replay_dir = dir.path().join("replays");
    let port = 8936;
    let cfg = ServerConfig {
        port,
        bind: "127.0.0.1".to_string(),
        db_path: dir.path().join("ladder.db"),
        replay_dir: replay_dir.clone(),
        viewer_dir: None,
        lanes: 1,
        min_bots: 2,
        house_bots: 8,
        spectate_delay_s: 0,
    };
    let mut match_cfg = MatchConfig::standard();
    match_cfg.match_max_s = 40;
    tokio::spawn(async move {
        Server::start(cfg, match_cfg).await.expect("server");
    });
    tokio::time::sleep(Duration::from_millis(600)).await;

    let url = format!("ws://127.0.0.1:{port}/ws/bot");
    // Host: a raider who creates the raid room.
    let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let (mut host_tx, mut host_rx) = ws.split();
    host_tx
        .send(Message::Text(
            json!({"type":"register","name":"raid-leader","decision_rate":1,"human":true,
                   "mode":"boss","lobby_action":"create"})
                .to_string(),
        ))
        .await
        .unwrap();
    let code = loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), host_rx.next())
            .await
            .expect("host hears back")
            .unwrap()
            .unwrap();
        let Message::Text(t) = msg else { continue };
        let v: serde_json::Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "lobby_joined" {
            assert_eq!(v["mode"], "boss");
            break v["lobby"].as_str().unwrap().to_string();
        }
    };

    // Guest: registers as a raider but claims the boss role in the lobby.
    let (ws2, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    let (mut boss_tx, mut boss_rx) = ws2.split();
    boss_tx
        .send(Message::Text(
            json!({"type":"register","name":"guest-boss","decision_rate":1,"human":true,
                   "mode":"boss","boss":true,"lobby_action":"join","lobby":code})
                .to_string(),
        ))
        .await
        .unwrap();

    // Host starts the raid by hand, casting the guest as the boss.
    let mut waiting = true;
    while waiting {
        let msg = tokio::time::timeout(Duration::from_secs(10), host_rx.next())
            .await
            .expect("host hears back")
            .unwrap()
            .unwrap();
        let Message::Text(t) = msg else { continue };
        let v: serde_json::Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "lobby_roster" {
            assert_eq!(v["members"].as_array().unwrap().len(), 2);
            host_tx
                .send(Message::Text(json!({"type":"lobby_start","action":"start"}).to_string()))
                .await
                .unwrap();
            waiting = false;
        }
    }

    // Both players see the raid start; the boss role lands on the guest.
    let mut host_role: Option<String> = None;
    let mut boss_role: Option<String> = None;
    let mut host_is_boss = false;
    let mut boss_is_boss = false;
    let started = tokio::time::Instant::now();
    while started.elapsed() < Duration::from_secs(20) && (host_role.is_none() || boss_role.is_none()) {
        tokio::select! {
            msg = host_rx.next() => {
                let Some(Ok(Message::Text(t))) = msg else { break };
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                if v["type"] == "match_start" {
                    assert_eq!(v["mode"], "boss");
                    host_is_boss = v["role"] == "boss";
                    host_role = v["role"].as_str().map(String::from);
                    let entrants: Vec<String> = v["bots"].as_array().unwrap().iter()
                        .filter_map(|b| b.as_str().map(String::from)).collect();
                    // The boss is the last entrant (its slot becomes the boss unit).
                    assert_eq!(entrants.last().map(String::as_str), Some("guest-boss"),
                        "cast boss sits in the last slot: {entrants:?}");
                }
                host_tx.send(Message::Text(active_action(&v).unwrap_or(json!({"tick":0})).to_string())).await.ok();
            }
            msg = boss_rx.next() => {
                let Some(Ok(Message::Text(t))) = msg else { break };
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                if v["type"] == "match_start" {
                    boss_is_boss = v["role"] == "boss";
                    boss_role = v["role"].as_str().map(String::from);
                }
                boss_tx.send(Message::Text(active_action(&v).unwrap_or(json!({"tick":0})).to_string())).await.ok();
            }
        }
    }
    assert!(!host_is_boss, "the host raided, not bossed");
    assert!(boss_is_boss, "the guest was cast as the boss");
    assert_eq!(host_role.as_deref(), Some("raider"));
    assert_eq!(boss_role.as_deref(), Some("boss"));

    // Play it out; the boss guest must survive longer than a walkover.
    let mut over: Option<(String, serde_json::Value)> = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    while tokio::time::Instant::now() < deadline && over.is_none() {
        tokio::select! {
            msg = host_rx.next() => {
                let Some(Ok(Message::Text(t))) = msg else { break };
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                if v["type"] == "match_over" { over = Some(("raid-leader".into(), v)); }
                else if v["type"].is_null() { host_tx.send(Message::Text(active_action(&v).unwrap().to_string())).await.ok(); }
            }
            msg = boss_rx.next() => {
                let Some(Ok(Message::Text(t))) = msg else { break };
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                if v["type"] == "match_over" { over = Some(("guest-boss".into(), v)); }
                else if v["type"].is_null() { boss_tx.send(Message::Text(active_action(&v).unwrap().to_string())).await.ok(); }
            }
        }
    }
    let (who, _o) = over.expect("the raid finished");

    // The raid replay verifies and is a boss-mode match.
    let replay_url = _o["replay"].as_str().expect("replay url");
    let replay_path = replay_dir.join(replay_url.trim_start_matches("/replays/"));
    let replay: abr_core::replay::Replay =
        serde_json::from_slice(&std::fs::read(&replay_path).unwrap()).unwrap();
    assert_eq!(replay.header.config.mode, abr_core::config::GameMode::Boss);
    assert!(replay.header.bot_names.last().map(|n| n == "guest-boss").unwrap_or(false),
        "guest-boss was the raid boss: {:?}", replay.header.bot_names);
    let verified = abr_core::replay::verify_replay(&replay).expect("raid replay verifies");
    assert!(verified.ticks > 0, "raid had substance ({who})");
}
