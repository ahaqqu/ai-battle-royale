# AI Battle Royale — Design Plan

**Status:** Design document. No implementation yet.
**Date:** 2026-09-26
**Deliverable this document describes:** A real-time battle royale game where every entrant is a player-written AI, with strict fog of war, built for spectating as much as for competing.

---

## 1. Vision

A 16-bot battle royale. Every "player" is an AI program someone wrote, connected from their own server. Matches are short (5–8 minutes), lethal, and fun to watch — live with commentary, or as shareable replay links. The bots play under strict fog of war: they see only what their character can see and hear, and everything else — memory, prediction, deception — is their own code's job.

The design principle printed on every decision in this doc:

> **Simple to watch. Hard to play.**

- *Simple to watch:* one main character per bot, glowing zone, readable projectiles, a kill feed. A first-time viewer understands the rules in 30 seconds.
- *Hard to play:* partial observability turns the game into a POMDP. The winning bot is the one with the best belief model of where everyone is — not the one with the fastest trigger. Skill shows up as reasoning quality, and reasoning under uncertainty is inherently diverse and unpredictable, which is exactly what an esport needs.

### 1.1 Research-grounded rules the design commits to

These come from a survey of ~12 prior "bring your own AI" games (Battlesnake, Screeps, Lux AI, Halite, RLBot, MIT Battlecode, Gladiabots, Terminal, Vindinium, Robocode Tank Royale, AIIDE StarCraft ladders). They are the load-bearing design constraints; changing any of them needs a better reason than taste:

1. **Fixed-tick simulation that renders as continuous.** The sim steps at 10Hz; the renderer interpolates at 60fps. No surveyed project succeeded with true continuous real-time on open infrastructure; every winner used fast fixed ticks that read as live sport on screen. Fixed ticks are also the only sane foundation for determinism and replays.
2. **Graceful degradation on bot timeout.** A bot that misses its reply deadline repeats its last action (momentum). Matches stay watchable — a slow bot loses visibly, it is never instantly disqualified mid-fight. Battlesnake's momentum model proved this is the right spectator behavior; Halite's instant elimination proved the opposite.
3. **The automated ladder ships on day one.** Games that launched without continuous match content (Tank Royale, CodeGame) or lost their single funding rail (Halite, Vindinium) died regardless of technical quality. The ladder is not a feature; it is the game's heartbeat and content engine.
4. **The simulation is published as a library.** `step()` plus `observe()` as an open crate (native + WASM) lets serious bot authors forward-simulate, build belief-space planners, and regression-test offline. This is the single biggest lever for deep AI and ecosystem growth (Battlesnake and Halite both proved it).
5. **Protocol simplicity drives adoption more than features.** Vindinium's dead-simple HTTP+JSON protocol spawned 10+ community SDKs in months. JSON over WebSocket for v1; binary formats only if profiling ever demands them.
6. **Spectating is decoupled from the client and runs in the browser.** URL-shareable replays are the highest-leverage spectator feature found in the genre (Lux's s3vis, Terminal's playground). The RLBot approach of coupling spectating to a heavyweight game client is the trap we invert.

---

## 2. Game design

### 2.1 Format

- **16 entrants per match**, each entrant controlled by one bot program.
- **Each entrant = 1 main character + 1 companion** (a pet / small robot), leashed to its main.
- **Main death = elimination.** Companion death = 20s respawn beside the main (tuning knob; see §11).
- Win condition: last main alive. Secondary ladder metric: kills + placement.
- Match length target: 5–8 minutes.
- Squad mode (multiple mains + shared companions) is deliberately out of scope for v1 but nothing in the protocol prevents it later — one bot program just controls more mains. The flagship mode stays solo-main forever, so the spectator's eye always has exactly one hero to follow per bot.

### 2.2 Main character kit

Per tick the bot issues, for its main:

- **Move vector:** direction + magnitude (0–1 of max speed). Position is continuous.
- **One action** (optional; movement is always allowed):
  - `fire` — projectile toward a point. Projectile has travel time; damage on impact.
  - `dash` — short burst of speed in the move direction, costs energy, emits audible noise.
  - `shield` — brief damage reduction, costs energy, slows movement by 30%.
  - `sprint` (toggle) — +40% speed, but footsteps become audible and it disables firing.

Starting values (all tuning knobs, §11): HP 100 · speed 140 u/s · fire cooldown 0.5s · projectile speed 420 u/s, damage 12 · dash cost 20 energy, 0.3s duration, speed ×2.2 · shield cost 15 energy, 1.0s, 70% damage reduction · energy pool 100, regen 10/s (only while not sprint/dash/shield active).

**The reaction-window rule:** every survivable threat must telegraph for ≥ 300–500ms before it can kill you. Projectiles at 420 u/s across typical engagement distances (400–800 units) give 1–2 seconds of flight time; abilities are telegraphed; nothing one-shots a full-HP main. This rule exists so that a bot with a ~100ms network round trip can still theoretically outplay anything — latency must never be the deciding skill, reasoning speed within the tick budget must be. "Fast response" is measured against the tick deadline, not against ping.

### 2.3 Companion kit

The companion is the entrant's **scouting and information tool** — mechanically it exists to extend the information envelope beyond the main's vision, at personal risk.

- **Leash:** companion must stay within 350 units of the main. The server clamps movement beyond the leash; bots that try to push past it waste ticks fighting the clamp.
- **Per tick:** move vector + one action (or `heel` — return to main).
- **Speed:** 170 u/s (faster than the main).
- **Vision:** 250u radius (smaller than the main's 450u) — it is a peeking tool, not a second screen.
- **HP:** 30. Cannot capture zones, cannot pick up loot, deals no damage. Its only combat value is as a bullet sponge and a distraction.
- **One ability — sonar ping:** reveals silhouettes of all entities within 600u of the companion for 3 seconds. Costs 25 of the companion's 50-energy pool, cooldown 15s. **Emits a loud noise** audible at 900u — the enemy now knows roughly where you scouted from. This is the game's signature noise-discipline tradeoff: information for position.
- **Auto-heel mode (opt-in, server-side):** bots that never issue companion commands get a default AI that keeps the companion near the main and pings on cooldown. Beginners are never punished for ignoring half their kit; strong bots never use it.

### 2.4 Map

- 3200×3200 units, continuous space, static layout.
- **The static layout is public knowledge.** All bots receive the map file (walls, cover, spawn points) and can plan rotations offline. Fog covers only *dynamic* things: players, companions, projectiles, pickups. Rationale: in BR the uncertainty that matters is *where enemies are*, not where walls are; hiding terrain just forces wasteful map-learning code and adds no interesting decisions.
- 16 spread spawn points (perimeter + interior mix), selected by hidden RNG so spawns can't be pre-computed.
- Cover geometry: walls that block vision and projectiles; low cover that blocks projectiles only; open lanes with ranged sightlines. Symmetric enough to be fair, asymmetric enough to have a meta (high-ground center vs. loot-rich corners).
- 24–32 pickups per match spawned by hidden RNG: HP kits (+35), energy packs (+40), weapon mods (fire cooldown −20% / projectile speed +15%). Pickup schedules are derived from the hidden seed — scouting has real value and the seed must never leak (§3.4).

### 2.5 Battle royale systems

- **Zone:** shrinking circle. The current circle and the **next circle are always published one phase ahead** (center, radius, lock time) — every bot can plan its rotation; the tension is executing under fog, not solving a geometry puzzle.
- Zone phases (starting values): radius 1600 → 1200 → 850 → 550 → 300 → 0; 45–60s per phase with a 10s warning between. Outside the circle: 2 HP/s → scaling to 8 HP/s in final phases.
- **Kill feed with killer identity:** `"7 eliminated 3"` — knowing *who* killed *whom* is inference fuel (§3.3).
- **Alive count** published every tick.
- No PvE, no crafting, no inventory management. The skill axes are positioning, information, and combat — keep the action space deep, not wide.

---

## 3. Observation model (strict fog of war)

### 3.1 Architecture: fog lives only in the observation layer

- The server simulates the **full world state** authoritatively: every entity's exact position, HP, and intent.
- What each bot receives is a **per-bot observation** — a pure function `observe(state, viewer_id) -> observation` computed by the server each tick.
- The fog of war therefore has **zero effect on determinism or replays**: replays record the full state (§5.3), and the viewer can render any perspective — the global map, or literally through bot #7's eyes.
- The sim core exposes `observe()` in the published library (§5.2), so bot authors can test their belief code against exact observations offline.

### 3.2 Senses, in tiers

**Self (always full detail):** both own units — position, velocity, facing, HP, energy, cooldowns, active statuses, and the full state of own pickups/mods.

**Vision (primary sense):**

| Source | Radius |
|---|---|
| Main character | 450u |
| Companion | 250u |

- Vision is blocked by walls (raycast line-of-sight); each unit's visible set is the union of both circles.
- Two detail tiers inside vision:
  - **< 300u from the sensing unit: full detail** — position, velocity, facing, HP, active statuses.
  - **300–450u: silhouette** — position only, coarse ("something hostile is there, unknown HP"). Creates real engage/avoid decisions at range.
- Own projectiles always visible; **enemy projectiles are visible only inside your vision** — dodging a shot you never saw is impossible by design, which is why audio exists (below).

**Audio (the information leak):** loud events emit pings audible far beyond vision, but coarse. Each audio event is `{kind, bearing, band}` — never an exact position.

| Event | Audible radius | Notes |
|---|---|---|
| Gunshot | 900u | The biggest position leak in the game |
| Dash | 500u | |
| Sonar ping (companion) | 900u | Loud by design — the tradeoff |
| Sprint footsteps | 200u | Only while sprint is active |
| Walking | ~silent | Stealth exists |

Distance bands: near (<250u), mid (250–600u), far (>600u). Bearings are quantized to 15° increments — precise enough to act on, coarse enough to stay uncertain.

**Global (everyone knows):** zone geometry + next zone, match timer, alive count, kill feed. Terrain is static public knowledge (§2.4).

### 3.3 What the server never sends

- Enemy (or ally-team) entities outside the viewer's senses — at any detail level.
- Enemy HP beyond the silhouette policy — silhouettes are position-only, full detail only inside 300u.
- Projectiles outside vision.
- **"Last seen" ghosts.** Strict fog: if you saw an enemy 20 seconds ago, remembering and extrapolating that is *your bot's job*. (This is the highest-skill-ceiling choice; the SDK helper below is the on-ramp.)
- **The RNG seed.** In a full-information game the seed is harmless; in a fog game it is radioactive — with it, a bot can locally regenerate spawn assignments, loot schedules, and zone centers, and the fog collapses. Live matches send map ID + match config only. The seed exists solely inside the replay file.
- Pickup spawn schedules (they derive from the hidden seed — scouting must have value).

### 3.4 The memory problem is the game

What this model produces, deliberately, is a POMDP: the bot must maintain a **belief state** — its own probability distribution over where everyone is — and update it from partial, noisy, aging evidence (vision, audio bearings, kill feed, alive count). The decision problems this creates are the intended depth of the game:

- **Memory & inference:** last-seen registry, velocity extrapolation, ghost aging ("he was pushing center 30s ago").
- **Sound-based triangulation:** fusing bearing-only pings from successive ticks into a threat map.
- **Information as a resource:** is it worth sending the companion to peek that ridge? Information gathering is a first-class action with a price (companion risk, sonar noise).
- **Engage vs. stealth:** firing reveals you to everyone within 900u. Every engagement is an information trade, not just a damage trade.
- **Third-partying:** hearing two bots fight is a timing decision — the classic BR drama, now a genuine reasoning problem.
- **Endgame compression:** as the zone shrinks, belief uncertainty is crushed into a tiny area; forced encounters happen under incomplete information.

Two competent bots holding different beliefs make legitimately different choices. That is the unpredictability the game is built to produce — there is no dominant line of play to converge on.

### 3.5 SDK helper: BeliefTracker

Strict fog is the server's contract. To keep the on-ramp gentle without capping the ceiling, the **official starter SDK ships an optional, purely client-side `BeliefTracker` module**:

- Last-seen registry with velocity extrapolation and ghost aging.
- Audio-event fusion into a coarse threat heat map.
- Optional: a discrete grid belief (e.g. 64×64) the bot can query alongside raw observations.

It is a library convenience, never a protocol guarantee: bots may use it, modify it, or write their own. The server's observation contract stays strict so belief quality remains the skill that separates bots.

### 3.6 Observation message (example)

```json
{
  "apiversion": 1,
  "tick": 1842,
  "deadline_ms": 50,
  "you": {
    "main": {
      "pos": [1204.5, 882.0], "vel": [35.2, -12.0], "facing": 31,
      "hp": 74, "energy": 61, "mods": {"fire_cooldown_pct": -20},
      "cooldown": {"fire": 0, "dash": 12, "shield": 0},
      "status": ["sprint"]
    },
    "companion": {
      "pos": [1301.0, 902.5], "vel": [0.0, 0.0], "facing": 87,
      "hp": 30, "energy": 40,
      "cooldown": {"sonar": 63}
    }
  },
  "seen": {
    "players": [
      {"id": 12, "pos": [1501.0, 903.0], "vel": [80.0, 0.0], "facing": 191,
       "hp": 61, "range": 295, "detail": "full"},
      {"id": 4, "pos": [1610.0, 640.0], "range": 428, "detail": "silhouette"}
    ],
    "companions": [
      {"id": 51, "owner": 12, "pos": [1540.0, 912.0], "range": 340, "detail": "silhouette"}
    ],
    "projectiles": [
      {"id": 55, "pos": [1410.0, 950.0], "vel": [-420.0, 90.0], "owner": 12, "owner_kind": "main"}
    ],
    "pickups": [
      {"id": 61, "pos": [1180.0, 700.0], "kind": "energy"}
    ]
  },
  "heard": [
    {"tick": 1841, "kind": "gunshot", "bearing": 210, "band": "far"},
    {"tick": 1842, "kind": "footstep", "bearing": 75, "band": "near"}
  ],
  "global": {
    "alive": 6,
    "kill_feed": [
      {"tick": 1790, "killer": 12, "victim": 3}
    ],
    "zone": {
      "center": [1600, 1600], "radius": 850,
      "next": {"center": [1620, 1440], "radius": 550, "locks_at_tick": 2100}
    },
    "map_id": "arena-1",
    "match_time_left_s": 132
  }
}
```

Notes: `bearing` is degrees from north, quantized to 15°. `range` is distance from the nearest sensing unit. `facing` is degrees. A silhouette entry omits `hp`/`vel`/`facing` entirely. `id` values are stable per entity per match.

---

## 4. Bot protocol

### 4.1 Shape

- Bots are **player-hosted WebSocket servers** (the Battlesnake hosting model): any language, any framework, a $5 VPS or even a browser tab is enough to compete.
- One persistent WS connection per bot per match. The engine pushes one observation message per tick; the bot replies with one action message.
- All bots receive their observation **simultaneously**; all actions for a tick are resolved **simultaneously** with documented, deterministic tie-breaks (§5.1). No initiative order exists.

### 4.2 Timing

- **Sim tick: 10Hz** (100ms). Render at 60fps with interpolation; spectating reads as continuous motion.
- **Reply deadline: 50ms** from the moment the observation is sent, published inside every message (`deadline_ms`). The remaining ~50ms of each tick is the engine's resolution window (movement, collisions, projectiles, zone, observation slicing for the next tick).
- **Slow decision rate (opt-in):** a bot declares at connect time that it acts every Nth tick (N = 2–10). Between its decision ticks, its last action set repeats. This is how remote/heavy bots degrade gracefully on a single-region server instead of stalling matches (and see §8.2 — latency fairness).

### 4.3 Timeout ladder (graceful degradation)

1. **Missed deadline → momentum:** the last action set repeats. The bot keeps playing; it just can't change its mind. A slow bot loses *visibly* — the show goes on.
2. **Chronic overruns → graduated penalties (AIIDE-style, punishing sustained slowness not spikes):** e.g. a bot loses the match if ≥1 reply exceeds 1s, or ≥30 replies exceed 200ms, or it misses ≥20% of its deadlines cumulatively. Exact thresholds are config, tuned so a normal GC pause never matters but a bot that simply can't keep up is retired without dragging the match.
3. **Disconnect / protocol violation / invalid action schema → forfeit.** Invalid *content* within a valid schema (e.g. aiming into a wall) is not a violation — the engine resolves it as a legal-but-bad action. Be permissive at the edges of legality, strict about the contract.
4. **Connection loss mid-match:** bot gets 10s of momentum-repeats before forfeit — enough to survive a transient blip, short enough that a dead bot doesn't zombie to placement.

### 4.4 Action message (example)

```json
{
  "apiversion": 1,
  "tick": 1842,
  "main": {
    "move": {"dir": 45, "throttle": 1.0},
    "action": {"type": "fire", "target": [1500.0, 900.0]},
    "intent": "pushing center-left, heard 12 near rocks"
  },
  "companion": {
    "move": {"dir": 90, "throttle": 1.0},
    "action": {"type": "sonar"}
  }
}
```

- `move.dir`: degrees; `throttle`: 0–1.
- `action` is optional (movement alone is always legal). One action per unit per tick.
- `intent` (main only, ≤64 chars) is the spectator "shout" — see §6.3. It is optional, rate-limited, and purely cosmetic; the engine never parses it.
- Unknown fields in bot replies are ignored (forward compatibility). Missing required fields = invalid schema → protocol violation ladder.
- **Versioning:** `apiversion` in every message both directions; the engine supports v1 only at launch and negotiates at connection time. Breaking changes bump the major version; additive fields within v1 never break bots.

### 4.5 Wire format

- **JSON for v1.** At 10Hz with ~3–6KB observations, bandwidth is trivial on both ends (§8.3), and human-readable wire format is why the simple protocols won the ecosystem race in prior art. Debuggable with `curl`-equivalent tooling is a feature, not a phase.
- FlatBuffers/binary is a documented *later option*, only if profiling against real ladder traffic ever demands it. The message schema stays the contract; the serialization is an implementation detail.

---

## 5. Determinism, the sim library, and replays

### 5.1 Deterministic simulation core

- **Rust crate**, one authoritative implementation compiled twice: native (match server) and WASM (web viewer, bot SDK) — the same code path guarantees bit-identical simulation on both ends.
- **Fixed-point math** (no floats in game state), **seeded PRNG** for all randomness (spawns, loot, zone jitter), **sorted iteration order** everywhere (no HashMap iteration in game logic). These three rules are what make `native == wasm` reproducible across platforms and what make replays exact.
- Simultaneous action resolution with documented tie-breaks, applied in a fixed order: movement (clamped by leash/walls) → dashes → projectile spawns → projectile flight + impacts → ability effects → zone damage → pickups → deaths. The order is part of the published rules, not an implementation detail.
- The tick deadline (§4.2) never influences the outcome: bot compute time affects only *which* actions arrive, never how they resolve. (Battlecode's rationale for deterministic compute meters, restated as a design rule for us.)

### 5.2 The published library contract

The sim core ships as an open crate exposing exactly:

- `step(state, actions) -> state` — advance one tick.
- `observe(state, viewer_id) -> observation` — the exact observation the server would send (§3).
- `load_map(map_id)`, `new_match(config, seed) -> state`.
- Local runner CLI: run bot-vs-bot matches offline, output replays.

**Fog and the library:** the library necessarily contains `observe()`, so bot authors *can* forward-simulate hypothetical worlds consistent with their observations — that is belief-space planning and it is a sanctioned strength. What they cannot do is recover the hidden seed from a live match, because live matches never send it (§3.3). The library also accepts a `--full-info` debug flag for local development; **ranked mode is always strict fog** — the debug flag exists only on the local runner, never on the ladder.

### 5.3 Replay format

- A replay = **map id + match config + seed + ordered per-tick action log.** Because the sim is deterministic, this file regenerates the *entire* match exactly — every projectile, every observation any bot ever received (via `observe(state, viewer_id)`).
- Files are tiny (KB-scale: actions compress extremely well) and are **served as static files** — no compute to watch a replay.
- A "fat" replay variant (pre-rendered frame snapshots every tick) may be added later for instant scrubbing; the canonical format is the thin one.
- **URL-shareable viewer:** every ladder match gets a replay link that opens the in-browser viewer at tick 0 (§6). This is the community content engine — matches must be shareable with one link from day one.

---

## 6. Spectating

### 6.1 Spectator model

- Viewers see the **full map** while bots see slices. That information asymmetry — *the audience knows the enemy is behind the wall; the bot doesn't* — is the entire entertainment engine of battle royale streaming, and the architecture gives it to us for free (fog is observation-layer only; replays and live streams carry full state).
- The spectator view also gets what the bots *don't*: the hidden info made visible — all HP bars, all projectiles, all pickups.
- Companions stay in frame near their mains; the viewer's eye always has one hero per bot to follow (the solo-main rule from §2.1 pays off here).

### 6.2 Viewer features

- **Global camera with auto-director:** cuts to the current action cluster (recent shots, kills, zone deaths); spectator can override and free-cam or follow any bot.
- **Player-cam replays:** re-watch any match rendered through one bot's observation — including what its companion saw. This is the game's signature content format: *"watch how bot #7 deduced the ambush."*
- **Slow-mo kill cams** and replay scrubbing (client-side; pure cosmetics, §7.3).
- **Live spectate with a 30–60s delay** (anti-cheat: the global view is a side channel — a live, un-delayed stream could feed enemy positions to a bot through a second connection; a delay makes cheating-by-watching useless in real time).

### 6.3 Mind-cam (belief overlay)

- Bots may publish, alongside actions, a small **debug channel** per tick: the 64×64 belief heat map and the `intent` string (§4.4).
- The viewer renders it as a toggleable "mind-cam" overlay: *"where bot #7 thinks everyone is."* Casters narrate it; discrepancies between belief and reality are themselves entertainment ("it's convinced 12 is in the rocks — 12 flanked").
- Opt-in, size-capped, rate-limited (updated at most every 5 ticks), and never parsed by the engine. A bot that lies in its heat map only lies to spectators — which is also funny, and allowed.

---

## 7. Tech stack

### 7.1 Confirmed shape (web-first)

| Layer | Choice | Why |
|---|---|---|
| Sim core | Rust crate, fixed-point, seeded PRNG | Bit-identical native + WASM from one codebase; no engine scheduler to fight |
| Match server | Rust binary embedding the core | Headless, tiny; the Battlesnake model |
| Bot gateway | Rust WebSocket server (tokio + axum/tungstenite) | Same binary as match server; one process, one box |
| Web viewer/client | **PixiJS v8** (WebGL, WebGPU when available) over WebSocket | Lowest spec floor; instant-load, embeddable, link-shareable viewing |
| Bot SDKs | Rust + TypeScript first (from the open schema), community ports later | Vindinium lesson: simple schema → ecosystem |
| Art | Flat/vector with neon glow accents | Identity from art direction, not rendering tech; survives stream compression |
| Replays | Static files + viewer re-runs the sim (WASM) in the browser | Zero server cost per view |

Rejected alternatives, for the record: Bevy (nondeterministic-by-design ECS plus breaking migrations every ~4 months — determinism is the one place we cannot carry churn); Unity (licensing friction on a server fleet; weak web-spectator story); Unreal (overkill for stylized 2D); Godot (the legitimate desktop fallback if one is ever wanted — the core embeds via GDExtension without modification).

### 7.2 Why this shape wins on a small budget

The server never renders anything. It steps the sim and slices observations; every pixel is the client's problem. That is why a 2 vCPU box can host a ladder (§8) and why spectators scale roughly like static-file downloads.

### 7.3 Art direction and game feel

- **Flat/vector neon:** shapes + glow accents; reads perfectly through Twitch compression; runs on integrated graphics.
- **All juice is client-only:** particles, trails, screen shake, hitstop, slow-mo, kill-cam framing. The viewer receives positions and events; the *feel* is generated locally. Because rendering is fully decoupled from the deterministic sim, no amount of cosmetic layering can ever desync a match — the Battlesnake viewer's safety property, restated.
- Skip fullscreen post-processing (bloom/blur chains) at the low end; gate it behind a quality setting.

---

## 8. Deployment on the 2 vCPU / 4GB VPS

### 8.1 What runs on the box

One Rust process (match engine + bot gateway + viewer/replay API + static file serving), plus the ladder scheduler. Postgres or SQLite for ladder/ELO and bot registry; replays as static files on disk. Nothing else.

### 8.2 Match cost and scheduling

- **Compute:** a 16-bot match at 10Hz — sim, wall raycasts (~300–500 rays/tick total), observation slicing — is low single-digit percent of one vCPU. Compute is never the constraint; the budget line is **network fan-out** (§8.3).
- **Concurrency: 1–2 simultaneous matches**, queued otherwise. At 5–8 min per match and 2 lanes, that is **400+ matches/day** of throughput — far more than a ladder needs. Even single-lane gives 200+/day of fresh replay content.
- **Ladder cadence:** every registered bot plays **~4–8 games/day** (config), matches drawn by ELO-proximity (±200 bands, widening with queue wait), ELO updated per match with placement adjustment for new bots. This guarantees every bot author has something new to watch every day — the retention property that kept Battlesnake alive.
- Queue priority: new bots get a burst of placement matches; veterans fill lanes as they arrive.
- Latency fairness: the region is published; bots pick a slower decision rate (§4.2) if their RTT is high. A single region is accepted for v1 — with the reaction-window rule (§2.2) keeping RTT non-decisive — and a second region is a known future cost if the community demands it.

### 8.3 Bandwidth budget (JSON v1)

- Per bot: ~3–6KB/tick × 10Hz ≈ 30–60KB/s → **~1MB/s aggregate for a full 16-bot match.** Sustainable.
- Per live spectator (global view): ~10–20KB/tick × 10Hz ≈ 100–200KB/s. At 5 concurrent live viewers that is another ~1MB/s.
- Consequences: **cap live spectators per match** (e.g. 10, with a "replay available immediately after" message) until delta-compression or a fan-out relay is needed; replays are static and effectively free. If JSON deltas later cut spectator cost 5–10×, raise the cap.
- 4GB RAM is comfortable: engine process is tens of MB; the rest is OS cache for replay files and the small DB.

### 8.4 Failure and abuse posture

- Bot timeouts never stall the tick (deadline collection, momentum fill) — a misbehaving bot wastes only its own match.
- Server-side caps per bot connection: message size, connect rate, reconnect frequency.
- The engine never executes bot code; player-hosted bots mean zero untrusted-code risk on the box (the sandboxing question is deferred until/unless uploaded-code ladders are added).

---

## 9. Roadmap

Each milestone has a demo-shaped acceptance criterion — the project is never more than one milestone from something playable or watchable.

**M1 — Sim core + local runner (playable headless)**
- Fixed-point core, map, mains + companions, projectiles, zone, loot, strict-fog `observe()`, momentum/timeout ladder, replay writing.
- Local CLI runs bot-vs-bot with two dumb scripted bots; `--full-info` debug mode.
- *Accept:* a full 16-bot match between reference bots runs headless to completion in < 1s of engine time, produces a replay, and re-simulating the replay reproduces it byte-identically.
- *Unblocks:* bot SDK work and viewer work in parallel.

**M2 — WASM + web viewer (watchable replays)**
- Core to WASM; PixiJS viewer: global camera, player-cam, scrubbing, auto-director v0.
- *Accept:* any M1 replay URL opens in a browser and scrubs correctly; a non-programmer understands what they're watching within 30 seconds.
- *Unblocks:* the shareable-content loop — the moment the project has its growth engine.

**M3 — Gateway + remote bots + ladder (the heartbeat)**
- WS gateway, connection/negotiation, timeout ladder live, match queue, ELO, replay store, daily schedule.
- *Accept:* two bots on two different VPSes play a scheduled ladder match on the production box; the ladder page updates; replay link appears automatically.
- *Unblocks:* real competition, day-one content.

**M4 — Art pass + juice + mind-cam (the spectacle)**
- Neon art direction, particles/trails/shake/hitstop/slow-mo kill cams, belief overlay + intent shouts, live delayed spectate.
- *Accept:* a casted showmatch looks and feels like a product, not a debug view.

**M5 — Community and events**
- Season finals on the ladder's ELO, casted events, highlight tooling (GIF/clip export), starter-kit polish (BeliefTracker docs), second-language SDKs from community demand.

---

## 10. Risks & mitigations

| Risk | Evidence | Mitigation |
|---|---|---|
| Single-server death (funding/attention lapse) | Halite (corporate sponsor left), Vindinium (solo maintainer, domain lapsed) | Open-source engine + exportable replay format from day one; the game is self-hostable; ladder data exportable |
| Bot latency unfairness on one region | Battlesnake's region work exists precisely because RTT matters | Reaction-window rule (§2.2) keeps RTT non-decisive; slower decision rates (§4.2); publish region; re-evaluate second region at community demand |
| Strict fog too hard for newcomers | Screeps' harsh CPU model scared newcomers; Gladiabots' editor kept them | SDK BeliefTracker (§3.5), `--full-info` local debugging, auto-heel companion mode (§2.3), reference bot tiers (dumb → reactive → belief-using) |
| No content engine | Tank Royale shipped no official ladder and stalled; CodeGame shipped infra without a game | Automated daily ladder from M3 (§8.2), URL-shareable replays from M2 — the project never depends on a person running matches |
| Cheating via live spectate | Global view is a side channel | 30–60s live delay (§6.2); replays public only post-match; belief channel is write-only from bots to viewers, never back |
| Seed leakage collapses fog | — | Seed never leaves the replay file (§3.3); ranked observe() is server-side only; periodic ladder audits re-run replays to verify determinism |
| JSON bandwidth surprise | — | §8.3 budget is measured against real messages in M3, before any format work; delta-compression is the planned response, not premature binary formats |

---

## 11. Open questions / tuning knobs

Nothing in this table blocks M1; all of these have working defaults and get tuned by playtest during M1–M2, before the ladder ever goes live.

| Knob | Starting value | Tuning question |
|---|---|---|
| Companion leash radius | 350u | Does scouting feel rewarded without making the companion a second main? |
| Companion respawn | 20s | How much should losing it hurt? |
| Sonar cooldown / radius / noise / cost | 15s / 600u / 900u / 25 energy (pool 50) | Is the information-for-position tradeoff taken often enough to create drama? |
| Shield slow / sprint penalty | −30% speed / firing disabled while sprinting | Do defensive and mobility options price correctly against their risks? |
| Vision radii | main 450u, companion 250u | Does the silhouette tier (300–450u) actually produce engage/avoid decisions? |
| Audio radii | gunshot 900u, dash 500u, footstep 200u | Is stealth viable but not dominant? |
| Bearing quantization | 15° | Coarse enough to preserve uncertainty? |
| Fire cooldown / projectile speed / damage | 0.5s / 420 u/s / 12 | Do fights last long enough to be watchable and short enough to be lethal? |
| HP / energy / regen | 100 / 100 / 10 per s | Does the energy economy force ability choices rather than ability spam? |
| Sprint speed bonus / footstep leak | +40% / audible at 200u | Is sprint the default or a gamble? |
| Zone phase schedule | 6 phases, 45–60s each | Does the endgame compress belief uncertainty at the right rate? |
| Loot density / types | 24–32 pickups, 3 kinds | Does scouting loot feel worth the risk? |
| Tick rate / deadline | 10Hz / 50ms | Do real remote bots under real RTT hit the deadline reliably at v1? (Measure in M3.) |
| Timeout ladder thresholds | §4.3 defaults | Do the penalties retire genuinely slow bots without ever punishing a GC pause? |
| Match length | 5–8 min target | Long enough for arcs, short enough for the ladder and for casting? |
| Live spectator cap | 10 concurrent | Raise when delta-compression lands (§8.3). |

---

*Prior-art research summarized in §1.1 draws on: Battlesnake (webhook protocol, momentum-on-timeout, open rules engine, daily ladders), Screeps (sandboxed in-game scripting, CPU buckets), Lux AI S3 (overage time pools, co-located bots, web replay viewer), Halite (stdin/stdout kits, seeded replays, single-sponsor death), RLBot (flatbuffers over TCP, decoupled spectating difficulties), MIT Battlecode (bytecode determinism), Gladiabots (no-code accessibility), Terminal (stdin/stdout JSON, recruiting-funded longevity), Vindinium (protocol simplicity → ecosystem), Robocode Tank Royale (open wire protocol, missing official ladder), AIIDE/BASIL (graduated timeout penalties).*