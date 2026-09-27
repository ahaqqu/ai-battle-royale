//! Match gateway + ladder (PLAN §4, §8): player-hosted bots connect over
//! WebSocket; the engine pushes one strict-fog observation per tick and
//! collects one action reply per bot (50ms deadline, momentum on misses).
//! A queue drafts matches between idle bots, the timeout ladder retires
//! chronically slow bots, finished matches write replays + update ELO, and
//! spectate sockets stream full-state frames on a delay (anti-cheat, §6.2).

pub mod db;
pub mod page;

use abr_core::config::MatchConfig;
use abr_core::engine::MatchEngine;
use abr_core::replay::ReplayRecorder;
use abr_core::types::BotInput;
/// String → WS text message (axum 0.8 uses Utf8Bytes).
fn tmsg(s: String) -> Message {
    Message::Text(s.into())
}

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as AxPath, State};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, Mutex};

/// One connected bot. The WS task owns the socket; the match task talks to
/// it through `out_tx` (server → bot) and `in_rx` (bot → server).
pub struct BotHandle {
    pub name: String,
    pub db_id: i64,
    pub decision_rate: u64,
    pub auto_heel: bool,
    pub connected: Arc<AtomicBool>,
    pub out_tx: mpsc::Sender<String>,
    pub in_rx: Arc<Mutex<mpsc::Receiver<BotMsg>>>,
}

pub struct BotMsg {
    pub client_tick: u64,
    pub input: BotInput,
    pub arrived: Instant,
}

#[derive(Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub db_path: PathBuf,
    pub replay_dir: PathBuf,
    pub viewer_dir: Option<PathBuf>,
    /// Concurrent match lanes (PLAN §8.2: 1–2).
    pub lanes: usize,
    /// Minimum connected bots before the queue drafts a match.
    pub min_bots: usize,
    /// Spectate delay in seconds (anti-cheat, PLAN §6.2).
    pub spectate_delay_s: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            port: 8321,
            db_path: PathBuf::from("ladder.db"),
            replay_dir: PathBuf::from("replays"),
            viewer_dir: None,
            lanes: 2,
            min_bots: 2,
            spectate_delay_s: 30,
        }
    }
}

pub struct Server {
    pub cfg: ServerConfig,
    pub db: Arc<db::Db>,
    pub config: MatchConfig,
    pub lobby: Arc<Mutex<Vec<Arc<BotHandle>>>>,
    pub spectate_tx: broadcast::Sender<String>,
    pub lane_count: Arc<tokio::sync::Semaphore>,
}

#[derive(Deserialize)]
struct RegisterMsg {
    #[serde(rename = "type")]
    _type: String,
    name: String,
    #[serde(default)]
    token: String,
    #[serde(default = "default_rate")]
    decision_rate: u64,
    #[serde(default)]
    auto_heel: bool,
}

fn default_rate() -> u64 {
    1
}

#[derive(Deserialize)]
struct ClientAction {
    #[serde(default)]
    tick: u64,
    #[serde(flatten)]
    input: BotInput,
}

impl Server {
    pub async fn start(cfg: ServerConfig, config: MatchConfig) -> anyhow::Result<()> {
        let db = Arc::new(db::Db::open(&cfg.db_path)?);
        std::fs::create_dir_all(&cfg.replay_dir).ok();
        let (spectate_tx, _) = broadcast::channel(1024);
        let server = Arc::new(Server {
            lane_count: Arc::new(tokio::sync::Semaphore::new(cfg.lanes)),
            cfg,
            db,
            config,
            lobby: Arc::new(Mutex::new(Vec::new())),
            spectate_tx,
        });

        // Scheduler: draft matches between idle bots.
        {
            let server = server.clone();
            tokio::spawn(async move {
                loop {
                    server.schedule_tick().await;
                    tokio::time::sleep(Duration::from_millis(2000)).await;
                }
            });
        }

        let net = server.clone();
        start_axum(net).await
    }

    /// Called every 2s: draft a match if enough idle bots and a free lane.
    async fn schedule_tick(&self) {
        if self.lane_count.available_permits() == 0 {
            return;
        }
        let mut lobby = self.lobby.lock().await;
        lobby.retain(|h| h.connected.load(Ordering::Relaxed));
        if lobby.len() < self.cfg.min_bots {
            return;
        }
        // ELO-proximity drafting (PLAN §8.2): sort by elo, take up to 8.
        let take = lobby.len().min(8);
        lobby.sort_by_key(|h| self.db.elo_of(&h.name));
        let drafted: Vec<Arc<BotHandle>> = lobby.drain(..take).collect();
        drop(lobby);

        let permit = self.lane_count.clone().acquire_owned().await.unwrap();
        let server = Arc::new(Self {
            cfg: self.cfg.clone(),
            db: self.db.clone(),
            config: self.config.clone(),
            lobby: self.lobby.clone(),
            spectate_tx: self.spectate_tx.clone(),
            lane_count: self.lane_count.clone(),
        });
        tokio::spawn(async move {
            run_match(server, drafted).await;
            drop(permit);
        });
    }
}

/// The 10Hz match loop (PLAN §4.2): obs at t=0, 50ms reply deadline, ~50ms
/// resolution window, simultaneous resolution.
async fn run_match(server: Arc<Server>, handles: Vec<Arc<BotHandle>>) {
    let n = handles.len();
    let names: Vec<String> = handles.iter().map(|h| h.name.clone()).collect();
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(42)
        | 1;
    let mut engine = MatchEngine::new(server.config.clone(), seed, &names);
    for (b, h) in handles.iter().enumerate() {
        engine.configure_bot(b as u32, h.decision_rate, h.auto_heel);
    }
    let mut recorder = ReplayRecorder::new(&engine, &names);

    // Tell the bots what they're in for.
    for h in &handles {
        let _ = h
            .out_tx
            .send(
                json!({
                    "type": "match_start",
                    "bot": h.name,
                    "bots": names,
                    "map_id": engine.config.map_id,
                    "deadline_ms": engine.config.deadline_ms,
                    "tick_rate": engine.config.tick_rate_hz,
                    "seedless": true,
                })
                .to_string(),
            )
            .await;
    }
    println!("▶ match started: {} entrants: {}", n, names.join(", "));

    let tick_ms: u64 = 1000 / server.config.tick_rate_hz.max(1) as u64;
    let deadline = Duration::from_millis(server.config.deadline_ms);
    let mut interval = tokio::time::interval(Duration::from_millis(tick_ms));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut disconnected = vec![false; n];

    while !engine.state.finished {
        interval.tick().await;
        let tick_start = Instant::now();

        // Push this tick's observation to every bot simultaneously.
        for (b, h) in handles.iter().enumerate() {
            let obs = engine.observe(b as u32);
            let _ = h
                .out_tx
                .send(serde_json::to_string(&obs).unwrap_or_default())
                .await;
        }

        // Reply deadline window (PLAN §4.2): anything arriving within the
        // deadline is on time; the tail of the window catches stragglers,
        // whose true latency feeds the timeout ladder (§4.3).
        tokio::time::sleep(deadline).await;
        drain_inputs(
            &mut engine,
            &mut recorder,
            &handles,
            tick_start,
            &mut disconnected,
        )
        .await;
        tokio::time::sleep(Duration::from_millis(
            tick_ms
                .saturating_sub(server.config.deadline_ms)
                .max(1)
                .saturating_sub(2),
        ))
        .await;
        drain_inputs(
            &mut engine,
            &mut recorder,
            &handles,
            tick_start,
            &mut disconnected,
        )
        .await;

        let events = engine.step_tick();
        recorder.record_tick(engine.state.digest());

        // Spectate bus: full state, everything the bots don't get.
        let frame = engine.spectator_frame(&events);
        if let Ok(json) = serde_json::to_string(&json!({
            "type": "frame",
            "frame": frame,
        })) {
            let _ = server.spectate_tx.send(json);
        }
    }

    // Persist: replay file + ladder + ELO.
    recorder.finish(&engine);
    let summary = abr_core::replay::build_summary(&engine, &names);
    let match_seq = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let file_name = format!("match-{match_seq}.json");
    let path = server.cfg.replay_dir.join(&file_name);
    std::fs::write(&path, recorder.to_json()).ok();
    let replay_url = format!("/replays/{file_name}");

    let places: std::collections::HashMap<String, i64> = summary
        .placements
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.clone(), i as i64 + 1))
        .collect();
    let ratings: Vec<(String, i64)> = names
        .iter()
        .map(|nm| (nm.clone(), server.db.elo_of(nm)))
        .collect();
    let elos = db::elo_update(&ratings, &places, 32.0);
    let results: Vec<(String, i64, i64)> = summary
        .placements
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.clone(), i as i64 + 1, p.kills as i64))
        .collect();
    server
        .db
        .record_match(seed, n as i64, summary.ticks, &replay_url, &results, &elos);
    println!(
        "✔ match over in {} ticks — winner: {:?} — replay: {}",
        summary.ticks, summary.winner, replay_url
    );

    // Notify the bots and requeue them.
    for (b, h) in handles.iter().enumerate() {
        let place = summary
            .placements
            .iter()
            .position(|p| p.bot == b as u32)
            .map(|i| i + 1)
            .unwrap_or(0);
        let _ = h
            .out_tx
            .send(
                json!({
                    "type": "match_over",
                    "place": place,
                    "replay": replay_url,
                    "new_elo": elos.iter().find(|(nm, _, _)| nm == &h.name).map(|(_, _, e)| *e),
                })
                .to_string(),
            )
            .await;
        if h.connected.load(Ordering::Relaxed) {
            server.lobby.lock().await.push(h.clone());
        }
    }
}

async fn drain_inputs(
    engine: &mut MatchEngine,
    recorder: &mut ReplayRecorder,
    handles: &[Arc<BotHandle>],
    tick_start: Instant,
    disconnected: &mut [bool],
) {
    for (b, h) in handles.iter().enumerate() {
        let mut rx = h.in_rx.lock().await;
        loop {
            match rx.try_recv() {
                Ok(msg) => {
                    let latency = msg.arrived.duration_since(tick_start).as_millis() as u64;
                    engine.submit(b as u32, msg.input.clone(), latency);
                    if msg.input.intent.is_some() || msg.input.belief.is_some() {
                        engine.submit_mind(
                            b as u32,
                            msg.input.intent.clone(),
                            msg.input.belief.clone(),
                        );
                    }
                    recorder.record_submit(b as u32, msg.input);
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    // Socket died: 10s of momentum, then forfeit (PLAN §4.3.4).
                    if !disconnected[b] {
                        disconnected[b] = true;
                        engine.disconnect(b as u32, engine.state.tick);
                    }
                    break;
                }
            }
        }
    }
}

async fn start_axum(server: Arc<Server>) -> anyhow::Result<()> {
    use axum::routing::get;
    use tower_http::services::{ServeDir, ServeFile};

    let replays_dir = server.cfg.replay_dir.clone();
    let replays_list = replays_dir.clone();

    let mut app = axum::Router::new()
        .route("/ladder", get(ladder_page))
        .route("/ws/bot", get(ws_bot_handler))
        .route("/ws/spectate", get(ws_spectate_handler))
        .route(
            "/api/standings",
            get(|State(s): State<Arc<Server>>| async move {
                axum::Json(s.db.standings()).into_response()
            }),
        )
        .route(
            "/api/map/{id}",
            get(|AxPath(id): AxPath<String>| async move {
                match abr_core::map::load_map(&id) {
                    Some(m) => axum::Json(m.to_wire()).into_response(),
                    None => "not found".into_response(),
                }
            }),
        )
        .route(
            "/api/matches",
            get(|State(s): State<Arc<Server>>| async move {
                axum::Json(s.db.recent_matches(50)).into_response()
            }),
        )
        .route(
            "/api/replays",
            get(move || async move {
                let mut items = vec![];
                if let Ok(rd) = std::fs::read_dir(&replays_list) {
                    for entry in rd.flatten() {
                        let p = entry.path();
                        if p.extension().is_some_and(|e| e == "json") {
                            let name = p
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                            items.push(json!({
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
            ServeDir::new(&replays_dir).append_index_html_on_directories(false),
        );

    if let Some(viewer) = &server.cfg.viewer_dir {
        let index = viewer.join("index.html");
        if index.exists() {
            app = app
                .fallback_service(ServeDir::new(viewer).not_found_service(ServeFile::new(index)));
        }
    }

    let app = app.with_state(server.clone());

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", server.cfg.port)).await?;
    println!(
        "▶ AI Battle Royale ladder server on http://0.0.0.0:{} (bots: /ws/bot, spectate: /ws/spectate)",
        server.cfg.port
    );
    axum::serve(listener, app).await?;
    Ok(())
}

async fn ladder_page(State(s): State<Arc<Server>>) -> impl IntoResponse {
    page::ladder_html(&s.db)
}

async fn ws_bot_handler(
    State(server): State<Arc<Server>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| on_bot_socket(server, socket))
}

async fn ws_spectate_handler(
    State(server): State<Arc<Server>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| on_spectate_socket(server, socket))
}

async fn on_bot_socket(server: Arc<Server>, ws: WebSocket) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    // Handshake: first message must be a register.
    let first = tokio::time::timeout(Duration::from_secs(10), ws_rx.next()).await;
    let reg: RegisterMsg = match first {
        Ok(Some(Ok(Message::Text(t)))) => serde_json::from_str(&t),
        _ => return,
    }
    .unwrap_or_else(|_| RegisterMsg {
        _type: String::new(),
        name: String::new(),
        token: String::new(),
        decision_rate: 1,
        auto_heel: false,
    });
    if reg.name.is_empty() || reg.name.len() > 32 {
        let _ = ws_tx
            .send(tmsg(
                json!({"type":"error","error":"invalid name"}).to_string(),
            ))
            .await;
        return;
    }
    if !server.db.verify_token(&reg.name, &reg.token) {
        let _ = ws_tx
            .send(tmsg(
                json!({"type":"error","error":"bad token"}).to_string(),
            ))
            .await;
        return;
    }
    let db_id = server.db.register_bot(&reg.name, &reg.token).unwrap_or(0);

    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);
    let (in_tx, in_rx) = mpsc::channel::<BotMsg>(64);
    let connected = Arc::new(AtomicBool::new(true));
    let connected2 = connected.clone();
    let handle = Arc::new(BotHandle {
        name: reg.name.clone(),
        db_id,
        decision_rate: reg.decision_rate.clamp(1, 10),
        // Auto-heel is opt-in per bot at registration (the companion AI
        // would otherwise overwrite the bot's own companion commands).
        auto_heel: reg.auto_heel,
        connected: connected.clone(),
        out_tx: out_tx.clone(),
        in_rx: Arc::new(Mutex::new(in_rx)),
    });

    let _ = ws_tx
        .send(tmsg(
            json!({"type":"registered","you": reg.name, "deadline_ms": server.config.deadline_ms})
                .to_string(),
        ))
        .await;
    server.lobby.lock().await.push(handle.clone());
    println!(
        "⇄ bot connected: {} (queue: {})",
        reg.name,
        server.lobby.lock().await.len()
    );

    // Reader: bot → server inputs.
    let reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Text(t) => {
                    if let Ok(action) = serde_json::from_str::<ClientAction>(&t) {
                        if in_tx
                            .send(BotMsg {
                                client_tick: action.tick,
                                input: action.input,
                                arrived: Instant::now(),
                            })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        connected2.store(false, Ordering::Relaxed);
    });

    // Writer: server → bot (observations + lifecycle events).
    while let Some(text) = out_rx.recv().await {
        if ws_tx.send(tmsg(text)).await.is_err() {
            break;
        }
    }
    connected.store(false, Ordering::Relaxed);
    reader.abort();
    server
        .lobby
        .lock()
        .await
        .retain(|h| h.name != reg.name || !h.connected.load(Ordering::Relaxed));
}

async fn on_spectate_socket(server: Arc<Server>, ws: WebSocket) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    let mut sub = server.spectate_tx.subscribe();
    let delay_ticks = server.cfg.spectate_delay_s * server.config.tick_rate_hz as u64;
    let mut buffer: VecDeque<(u64, String)> = VecDeque::new();

    // Anti-cheat delay (PLAN §6.2): frames stream out `delay` behind live.
    let sender = tokio::spawn(async move {
        loop {
            match sub.recv().await {
                Ok(json) => {
                    let tick = serde_json::from_str::<serde_json::Value>(&json)
                        .ok()
                        .and_then(|v| v["frame"]["tick"].as_u64())
                        .unwrap_or(0);
                    buffer.push_back((tick, json));
                    if let Some(&(front_tick, _)) = buffer.front() {
                        if tick.saturating_sub(front_tick) >= delay_ticks {
                            if let Some((_, out)) = buffer.pop_front() {
                                if ws_tx.send(tmsg(out)).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    // Keep the socket's read side alive until the client leaves.
    while let Some(Ok(msg)) = ws_rx.next().await {
        if matches!(msg, Message::Close(_)) {
            break;
        }
    }
    sender.abort();
}
