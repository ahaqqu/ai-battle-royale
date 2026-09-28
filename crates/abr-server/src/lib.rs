//! Match gateway + ladder (PLAN §4, §8): player-hosted bots connect over
//! WebSocket; the engine pushes one strict-fog observation per tick and
//! collects one action reply per bot (50ms deadline, momentum on misses).
//! A queue drafts matches between idle bots, the timeout ladder retires
//! chronically slow bots, finished matches write replays + update ELO, and
//! spectate sockets stream full-state frames on a delay (anti-cheat, §6.2).

pub mod db;
pub mod house;
pub mod page;

use abr_core::config::{GameMode, MatchConfig};
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
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    /// A human at the viewer (queued by the play client) — triggers house fill.
    pub human: bool,
    /// An in-process house bot (never re-queued, hidden from standings).
    pub house: bool,
    /// Which match mode this entrant queued for (royale unless it asked).
    pub mode: GameMode,
    /// This entrant wants to BE the boss in a Slain-the-Boss raid.
    pub wants_boss: bool,
    /// Private lobby this entrant belongs to, if any: members are never
    /// drafted by the public queue — their host starts the match by hand.
    /// Cleared when their lobby match starts (afterwards they requeue publicly).
    pub lobby: std::sync::Mutex<Option<String>>,
    pub connected: Arc<AtomicBool>,
    pub out_tx: mpsc::Sender<String>,
    pub in_rx: Arc<Mutex<mpsc::Receiver<BotMsg>>>,
}

impl BotHandle {
    /// The room code this entrant is waiting in, if any.
    pub fn lobby_code(&self) -> Option<String> {
        self.lobby.lock().expect("lobby lock").clone()
    }

    pub fn set_lobby(&self, code: Option<String>) {
        *self.lobby.lock().expect("lobby lock") = code;
    }
}

/// A private room created over the bot socket: a shareable code, a roster of
/// invited/joined players, and a host who starts the match when everyone is in.
/// Works for both modes — a royale lobby is just a match you picked the roster
/// for, and a boss lobby casts its last member (or the built-in brain) as boss.
pub struct Lobby {
    pub code: String,
    pub host: Arc<BotHandle>,
    pub mode: GameMode,
    /// Boss-cannon raid (`raid_size` entrants, boss last) vs royale.
    pub boss_raid: bool,
    pub members: Vec<Arc<BotHandle>>,
}

/// What a registering socket asked to do with lobbies.
enum LobbyIntent {
    None,
    Create { boss: bool },
    Join { code: String },
}

pub struct BotMsg {
    pub client_tick: u64,
    pub input: BotInput,
    pub arrived: Instant,
}

#[derive(Clone)]
pub struct ServerConfig {
    pub port: u16,
    /// Address to listen on: "0.0.0.0" (default) or "127.0.0.1" when a reverse
    /// proxy on the same host fronts the server.
    pub bind: String,
    pub db_path: PathBuf,
    pub replay_dir: PathBuf,
    pub viewer_dir: Option<PathBuf>,
    /// Concurrent match lanes (PLAN §8.2: 1–2).
    pub lanes: usize,
    /// Minimum connected bots before the queue drafts a match.
    pub min_bots: usize,
    /// When a human is queued, top the match up to 8 entrants with at most
    /// this many in-process house bots (0 disables solo play entirely).
    pub house_bots: usize,
    /// Spectate delay in seconds (anti-cheat, PLAN §6.2).
    pub spectate_delay_s: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            port: 8321,
            bind: "0.0.0.0".to_string(),
            db_path: PathBuf::from("ladder.db"),
            replay_dir: PathBuf::from("replays"),
            viewer_dir: None,
            lanes: 2,
            min_bots: 2,
            house_bots: 8,
            spectate_delay_s: 30,
        }
    }
}

pub struct Server {
    pub cfg: ServerConfig,
    pub db: Arc<db::Db>,
    pub config: MatchConfig,
    pub lobby: Arc<Mutex<Vec<Arc<BotHandle>>>>,
    /// Private rooms by share code (uppercase alphanumeric, e.g. "K7QP").
    pub lobbies: Arc<Mutex<HashMap<String, Lobby>>>,
    pub lobby_seq: Arc<AtomicU64>,
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
    #[serde(default)]
    human: bool,
    /// "royale" (default) or "boss" — queue for a Slain-the-Boss raid.
    #[serde(default)]
    mode: String,
    /// Register AS the raid boss: the server casts this entrant as the boss
    /// of the next raid instead of spawning the built-in boss brain.
    #[serde(default)]
    boss: bool,
    /// Lobby intent: "create" the room (you become host) or "join" `lobby`.
    #[serde(default)]
    lobby_action: String,
    /// Room code for `lobby_action: "join"`.
    #[serde(default)]
    lobby: String,
}

/// A message from a lobby host (or the viewer UI) on an established socket.
#[derive(Deserialize)]
struct LobbyCmd {
    #[serde(rename = "type")]
    _type: String,
    /// Only "start" for now; any other value is ignored.
    #[serde(default)]
    action: String,
    /// Entrants to fill the match up to (solo host); server clamps.
    #[serde(default)]
    fill: Option<usize>,
    /// In a boss lobby, start this member as the boss ("boss" or the name);
    /// absent = the built-in boss brain.
    #[serde(default)]
    boss: Option<String>,
}

fn default_rate() -> u64 {
    1
}

/// House-fill ceiling for a royale lobby (same 8 as the public queue).
const HOUSE_MATCH_MAX: usize = 8;

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
            lobbies: Arc::new(Mutex::new(HashMap::new())),
            lobby_seq: Arc::new(AtomicU64::new(0)),
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

    /// Called every 2s: draft a match between idle bots when a lane is free.
    /// Slain-the-Boss raids are drafted first: any queued boss-mode entrant
    /// (or a registered boss AI) starts a raid, topped up with house bots.
    /// Members of a private lobby are never drafted here — their host starts
    /// the match by hand (`start_lobby`).
    async fn schedule_tick(self: &Arc<Self>) {
        if self.lane_count.available_permits() == 0 {
            return;
        }
        let mut lobby = self.lobby.lock().await;
        // Private rooms are invisible to matchmaking: the host decides when
        // (and with whom) the match starts.
        lobby.retain(|h| h.lobby_code().is_none());
        lobby.retain(|h| h.connected.load(Ordering::Relaxed));
        let raid_queued = lobby.iter().any(|h| h.mode == GameMode::Boss || h.wants_boss);
        let raid = if raid_queued {
            self.draft_boss_raid(&mut lobby)
        } else {
            None
        };
        if let Some((drafted, config)) = raid {
            drop(lobby);
            self.spawn_match(drafted, config).await;
            return;
        }
        // A queued human wants a match NOW — house bots make up the numbers,
        // so solo play never waits for other bots to connect (README "play live").
        let has_human = lobby.iter().any(|h| h.human);
        if lobby.is_empty() || (lobby.len() < self.cfg.min_bots && !has_human) {
            return;
        }
        // ELO-proximity drafting (PLAN §8.2): sort by elo, take up to 8.
        lobby.sort_by_key(|h| self.db.elo_of(&h.name));
        let take = lobby.len().min(8);
        let mut drafted: Vec<Arc<BotHandle>> = lobby.drain(..take).collect();
        drop(lobby);
        if has_human {
            let want = 8usize.min(drafted.len() + self.cfg.house_bots);
            while drafted.len() < want {
                let brain = house::HOUSE_ROSTER[drafted.len() % house::HOUSE_ROSTER.len()];
                drafted.push(house::spawn(&self.db, brain));
            }
        }

        let config = self.config.clone();
        self.spawn_match(drafted, config).await;
    }

    /// Take a lane permit and run `drafted` with `config` in the background.
    async fn spawn_match(self: &Arc<Self>, drafted: Vec<Arc<BotHandle>>, config: MatchConfig) {
        let permit = self.lane_count.clone().acquire_owned().await.unwrap();
        let server = self.clone();
        tokio::spawn(async move {
            run_match(server, drafted, config).await;
            drop(permit);
        });
    }

    /// Generate a share code nobody is using: 4 chars from a no-lookalike
    /// alphabet ("K7QP"), retried on the (vanishingly rare) collision.
    async fn new_lobby_code(&self) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
        loop {
            let n = self.lobby_seq.fetch_add(1, Ordering::Relaxed);
            let mut seed = n
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos() as u64)
                    .unwrap_or(7);
            seed |= 1;
            let code: String = (0..4)
                .map(|_| {
                    // xorshift: cheap, deterministic given the seed, no rand dep.
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    ALPHABET[(seed % ALPHABET.len() as u64) as usize] as char
                })
                .collect();
            if !self.lobbies.lock().await.contains_key(&code) {
                return code;
            }
        }
    }

    /// Register a socket into either the public queue or a private lobby.
    /// Lobby membership is decided here, atomically with the queue push, so a
    /// lobby-bound entrant is never visible to the public scheduler.
    async fn admit(
        self: &Arc<Self>,
        handle: Arc<BotHandle>,
        intent: LobbyIntent,
        ws_tx: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    ) -> anyhow::Result<()> {
        let mode = handle.mode;
        let roster_msg = |lobby: &Lobby, you: &str| {
            json!({
                "type": "lobby_joined",
                "lobby": lobby.code,
                "host": lobby.host.name,
                "you": you,
                "mode": if lobby.mode == GameMode::Boss { "boss" } else { "royale" },
                "members": lobby.members.iter().map(|m| m.name.clone()).collect::<Vec<_>>(),
            })
            .to_string()
        };
        match intent {
            LobbyIntent::None => {
                if handle.wants_boss {
                    let _ = ws_tx
                        .send(tmsg(
                            json!({"type":"queued_as_boss","hint":"waiting for a raid host"})
                                .to_string(),
                        ))
                        .await;
                }
                self.lobby.lock().await.push(handle);
                Ok(())
            }
            LobbyIntent::Create { boss } => {
                let code = self.new_lobby_code().await;
                handle.set_lobby(Some(code.clone()));
                let lobby = Lobby {
                    code: code.clone(),
                    host: handle.clone(),
                    mode,
                    boss_raid: boss,
                    members: vec![handle.clone()],
                };
                let msg = roster_msg(&lobby, &handle.name);
                self.lobbies.lock().await.insert(code.clone(), lobby);
                println!("⇄ lobby {code} created by {}", handle.name);
                let _ = ws_tx.send(tmsg(msg)).await;
                Ok(())
            }
            LobbyIntent::Join { code } => {
                let code = code.trim().to_uppercase();
                let mut lobbies = self.lobbies.lock().await;
                let Some(lobby) = lobbies.get_mut(&code) else {
                    drop(lobbies);
                    let _ = ws_tx
                        .send(tmsg(
                            json!({"type":"error","error":"no such lobby"}).to_string(),
                        ))
                        .await;
                    return Ok(());
                };
                let cap = if lobby.boss_raid {
                    self.config.boss.raid_size.max(2) as usize
                } else {
                    HOUSE_MATCH_MAX
                };
                if lobby.members.len() >= cap {
                    let n = lobby.members.len();
                    drop(lobbies);
                    let _ = ws_tx
                        .send(tmsg(
                            json!({"type":"error","error":format!("lobby is full ({n}/{cap})")})
                                .to_string(),
                        ))
                        .await;
                    return Ok(());
                }
                handle.set_lobby(Some(code.clone()));
                let names: Vec<String> = lobby
                    .members
                    .iter()
                    .map(|m| m.name.clone())
                    .chain(std::iter::once(handle.name.clone()))
                    .collect();
                lobby.members.push(handle.clone());
                let msg = json!({
                    "type": "lobby_joined",
                    "lobby": code,
                    "host": lobby.host.name,
                    "you": handle.name,
                    "mode": if lobby.mode == GameMode::Boss { "boss" } else { "royale" },
                    "members": names,
                })
                .to_string();
                let host_msg = json!({
                    "type": "lobby_roster",
                    "lobby": code,
                    "host": lobby.host.name,
                    "mode": if lobby.mode == GameMode::Boss { "boss" } else { "royale" },
                    "members": names,
                })
                .to_string();
                // Everyone already in the room hears the new roster; the joiner
                // has `lobby_joined` (which carries the same roster).
                let others: Vec<Arc<BotHandle>> = lobby.members[..lobby.members.len() - 1].to_vec();
                drop(lobbies);
                let _ = ws_tx.send(tmsg(msg)).await;
                for m in others {
                    let _ = m.out_tx.send(host_msg.clone()).await;
                }
                println!("⇄ lobby {code}: {} joined ({names:?})", handle.name);
                Ok(())
            }
        }
    }

    /// The lobby host hits start: the room's members become the match roster —
    /// house bots fill it up to `fill` (solo testing) and, in a boss lobby, the
    /// boss is the member the host named (`boss: "boss"`/a name) or the
    /// built-in brain. The room is consumed: one lobby = one match.
    async fn start_lobby(
        self: &Arc<Self>,
        host: &Arc<BotHandle>,
        fill: Option<usize>,
        boss_name: Option<String>,
    ) -> Result<(), String> {
        let code = host.lobby_code().ok_or("you are not in a lobby")?;
        let mut lobbies = self.lobbies.lock().await;
        let Some(lobby) = lobbies.remove(&code) else {
            return Err("lobby is gone".into());
        };
        if lobby.host.name != host.name {
            // Put the room back — only its creator starts the match.
            lobbies.insert(code, lobby);
            return Err("only the host can start".into());
        }
        if lobby.members.len() < 2 && self.cfg.house_bots == 0 {
            // The engine needs two entrants; with house fill off, one is not a match.
            lobbies.insert(code, lobby);
            return Err("need at least 2 players".into());
        }
        drop(lobbies);

        let mut drafted: Vec<Arc<BotHandle>> = lobby
            .members
            .iter()
            .filter(|m| m.connected.load(Ordering::Relaxed))
            .cloned()
            .collect();
        if !drafted.iter().any(|m| m.name == host.name) {
            return Err("host is disconnected".into());
        }
        if drafted.len() < 2 && self.cfg.house_bots == 0 {
            return Err("need at least 2 players".into());
        }

        let config = MatchConfig {
            mode: lobby.mode,
            ..self.config.clone()
        };

        if lobby.mode == GameMode::Boss {
            let raid_size = config.boss.raid_size.max(2) as usize;
            // Who leads the raid: the host's pick, else a boss-mode member,
            // else the built-in brain. `"ai"` forces the built-in brain.
            let boss_name = boss_name.unwrap_or_default();
            let boss_idx = if boss_name.eq_ignore_ascii_case("ai") {
                None
            } else if boss_name.is_empty() || boss_name.eq_ignore_ascii_case("boss") {
                // No pick (or "boss"): a member who claimed the role, else brain.
                drafted.iter().position(|m| m.wants_boss)
            } else {
                drafted
                    .iter()
                    .position(|m| m.name.eq_ignore_ascii_case(&boss_name))
            };
            let boss = match boss_idx {
                Some(i) => drafted.remove(i),
                None => house::spawn_boss(&self.db),
            };
            while drafted.len() < raid_size - 1 {
                let brain = house::HOUSE_ROSTER[drafted.len() % house::HOUSE_ROSTER.len()];
                drafted.push(house::spawn(&self.db, brain));
            }
            drafted.truncate(raid_size - 1);
            drafted.push(boss);
            let msg = json!({"type":"lobby_started","lobby":code}).to_string();
            for m in lobby.members.iter() {
                let _ = m.out_tx.send(msg.clone()).await;
            }
            println!(
                "▶ lobby {code} starting raid: {} (boss: {})",
                drafted.len(),
                drafted.last().map(|h| h.name.clone()).unwrap_or_default()
            );
            self.spawn_match(drafted, config).await;
            return Ok(());
        }

        // Royale lobby: fill to `fill` (default: house-filled 8, 0 disables).
        let want = fill
            .unwrap_or(8)
            .clamp(drafted.len(), HOUSE_MATCH_MAX)
            .min(drafted.len() + self.cfg.house_bots);
        while drafted.len() < want {
            let brain = house::HOUSE_ROSTER[drafted.len() % house::HOUSE_ROSTER.len()];
            drafted.push(house::spawn(&self.db, brain));
        }
        let msg = json!({"type":"lobby_started","lobby":code}).to_string();
        for m in lobby.members.iter() {
            let _ = m.out_tx.send(msg.clone()).await;
        }
        println!("▶ lobby {code} starting royale: {} entrants", drafted.len());
        self.spawn_match(drafted, config).await;
        Ok(())
    }

    /// Draft one Slain-the-Boss raid from the queue: raiders = boss-mode
    /// entrants topped up to `raid_size` with house bots, boss slot = a
    /// registered boss AI if one queued, else the built-in boss brain.
    fn draft_boss_raid(
        &self,
        lobby: &mut Vec<Arc<BotHandle>>,
    ) -> Option<(Vec<Arc<BotHandle>>, MatchConfig)> {
        let raid_size = self.config.boss.raid_size.max(2) as usize;
        // The raiders (boss-mode queuers; a registered boss takes no raider slot).
        let mut drafted: Vec<Arc<BotHandle>> = lobby
            .iter()
            .position(|h| h.wants_boss)
            .map(|bi| {
                lobby
                    .iter()
                    .enumerate()
                    .filter(|(i, h)| *i != bi && h.mode == GameMode::Boss)
                    .map(|(_, h)| h.clone())
                    .collect()
            })
            .unwrap_or_else(|| {
                lobby
                    .iter()
                    .filter(|h| h.mode == GameMode::Boss)
                    .cloned()
                    .collect()
            });
        drafted.truncate(raid_size - 1);
        // The boss: a registered boss AI, else the built-in brain.
        let boss_handle = match lobby.iter().position(|h| h.wants_boss) {
            Some(bi) => lobby.remove(bi),
            None => house::spawn_boss(&self.db),
        };
        lobby.retain(|h| h.mode != GameMode::Boss);

        while drafted.len() < raid_size - 1 {
            let brain = house::HOUSE_ROSTER[drafted.len() % house::HOUSE_ROSTER.len()];
            drafted.push(house::spawn(&self.db, brain));
        }
        // The boss entrant is always last (that slot becomes UnitKind::Boss).
        drafted.push(boss_handle);

        let config = MatchConfig {
            mode: GameMode::Boss,
            ..self.config.clone()
        };
        Some((drafted, config))
    }
}

/// The 10Hz match loop (PLAN §4.2): obs at t=0, 50ms reply deadline, ~50ms
/// resolution window, simultaneous resolution. `config` decides the mode —
/// royale drafts pass the server default, raids pass a Boss-mode clone.
async fn run_match(server: Arc<Server>, handles: Vec<Arc<BotHandle>>, config: MatchConfig) {
    let n = handles.len();
    let names: Vec<String> = handles.iter().map(|h| h.name.clone()).collect();
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(42)
        | 1;
    let mut engine = MatchEngine::new(config.clone(), seed, &names);
    for (b, h) in handles.iter().enumerate() {
        engine.configure_bot(b as u32, h.decision_rate, h.auto_heel);
    }
    let mut recorder = ReplayRecorder::new(&engine, &names);
    let boss_bot = if config.mode == GameMode::Boss {
        Some(n - 1)
    } else {
        None
    };

    // Tell the bots what they're in for.
    for (b, h) in handles.iter().enumerate() {
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
                    "mode": if config.mode == GameMode::Boss { "boss" } else { "royale" },
                    "role": if Some(b) == boss_bot { "boss" } else { "raider" },
                })
                .to_string(),
            )
            .await;
    }
    println!(
        "▶ match started: {} entrants: {}{}",
        n,
        names.join(", "),
        if boss_bot.is_some() {
            format!("  [SLAIN THE BOSS — boss: {}]", names[n - 1])
        } else {
            String::new()
        }
    );

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
        if h.connected.load(Ordering::Relaxed) && !h.house {
            // The room is consumed by its match: members go back to the
            // public queue (a fresh lobby is a fresh code).
            h.set_lobby(None);
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

    let listener = tokio::net::TcpListener::bind((server.cfg.bind.as_str(), server.cfg.port)).await?;
    println!(
        "▶ GUNBATTE ROYALE ladder server on http://{}:{} (bots: /ws/bot, spectate: /ws/spectate)",
        server.cfg.bind, server.cfg.port
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
        human: false,
        mode: String::new(),
        boss: false,
        lobby_action: String::new(),
        lobby: String::new(),
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
    let mode = if reg.mode.eq_ignore_ascii_case("boss") {
        GameMode::Boss
    } else {
        GameMode::Royale
    };
    // Lobby intent decides queue vs private room: "boss" as a *lobby* action
    // means "make this a raid" (the host is its first raider, not the boss).
    let intent = match reg.lobby_action.as_str() {
        "create" => LobbyIntent::Create {
            boss: mode == GameMode::Boss,
        },
        "join" => LobbyIntent::Join {
            code: reg.lobby.clone(),
        },
        _ => LobbyIntent::None,
    };
    let handle = Arc::new(BotHandle {
        name: reg.name.clone(),
        db_id,
        decision_rate: reg.decision_rate.clamp(1, 10),
        // Auto-heel is opt-in per bot at registration (the companion AI
        // would otherwise overwrite the bot's own companion commands).
        auto_heel: reg.auto_heel,
        human: reg.human,
        house: false,
        mode,
        wants_boss: reg.boss,
        lobby: std::sync::Mutex::new(None),
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
    if let Err(e) = server.admit(handle.clone(), intent, &mut ws_tx).await {
        println!("⚠ admit failed for {}: {e}", reg.name);
    }
    println!(
        "⇄ bot connected: {} (queue: {})",
        reg.name,
        server.lobby.lock().await.len()
    );

    // The host's lobby commands arrive on the same socket as its inputs.
    let (lobby_tx, mut lobby_rx) = mpsc::channel::<LobbyCmd>(8);

    // Reader: bot → server inputs (+ lobby_* control messages).
    let reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Text(t) => {
                    // Control messages are identified by their `type`.
                    let v: Option<serde_json::Value> = serde_json::from_str(&t).ok();
                    if let Some(ty) = v.as_ref().and_then(|v| v["type"].as_str()) {
                        if ty == "lobby_start" {
                            if let Ok(cmd) = serde_json::from_str::<LobbyCmd>(&t) {
                                if lobby_tx.send(cmd).await.is_err() {
                                    break;
                                }
                            }
                            continue;
                        }
                    }
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

    let host = handle.clone();
    let server2 = server.clone();
    let out_tx2 = out_tx.clone();
    tokio::spawn(async move {
        while let Some(cmd) = lobby_rx.recv().await {
            if cmd.action != "start" {
                continue;
            }
            if let Err(e) = server2
                .start_lobby(&host, cmd.fill, cmd.boss.clone())
                .await
            {
                let _ = out_tx2
                    .send(json!({"type":"error","error":e}).to_string())
                    .await;
            }
        }
    });

    // Writer: server → bot (observations + lifecycle events).
    while let Some(text) = out_rx.recv().await {
        if ws_tx.send(tmsg(text)).await.is_err() {
            break;
        }
    }
    connected.store(false, Ordering::Relaxed);
    reader.abort();
    let code = handle.lobby_code();
    server.lobby.lock().await.retain(|h| !Arc::ptr_eq(h, &handle));
    // A disconnecting member leaves the room; an empty room (or the host
    // leaving) closes it so nobody waits on a start that can never come.
    if let Some(code) = code {
        let mut lobbies = server.lobbies.lock().await;
        if let Some(l) = lobbies.get_mut(&code) {
            l.members.retain(|m| !Arc::ptr_eq(m, &handle));
            if Arc::ptr_eq(&l.host, &handle) {
                let msg = json!({"type":"lobby_closed","lobby":code,"reason":"host left"}).to_string();
                let members = l.members.clone();
                for m in &members {
                    m.set_lobby(None);
                    let _ = m.out_tx.send(msg.clone()).await;
                }
                lobbies.remove(&code);
                println!("⇄ lobby {code} closed (host left)");
            } else if let Some(l) = lobbies.get(&code) {
                if l.members.is_empty() {
                    lobbies.remove(&code);
                    println!("⇄ lobby {code} closed (empty)");
                } else {
                    println!("⇄ lobby {code}: {} left", handle.name);
                }
            }
        }
        drop(lobbies);
    }
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
