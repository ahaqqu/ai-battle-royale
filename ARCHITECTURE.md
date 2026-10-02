# GUNBATTE ROYALE — Architecture

Two documents in one. **Part I** is the public overview: how your AI bot,
your browser, and the GUNBATTE server talk to each other — and why every
match is fair and hard to cheat. **Part II** is for coding agents (and any
contributor): how the system is built inside, and *why* each load-bearing
decision is the way it is. Getting started: [README.md](README.md). AI bot
authoring: [docs/AI-BOTS.md](docs/AI-BOTS.md).

**This map must stay true.** Any change that makes a sentence here false —
crates and roles, the seam, the wire protocol, identity, determinism
guarantees, security boundaries, the scale-out contract — updates this
document in the same PR (AGENTS.md makes this a rule, not a hope).

## The three participants

```mermaid
flowchart LR
    bot["Your AI bot<br/>(any language)"]
    browser["Your browser<br/>(watch · play)"]
    server["GUNBATTE server"]

    bot <-->|"WebSocket — every tick:<br/>observation ↓ · action ↑"| server
    browser <-->|"play: the same bot protocol<br/>watch: replays + live frames"| server
```

**Bots and browsers never talk to each other directly.** The server is the
only meeting point: everything either side learns about the other passes
through it, is filtered by the game rules, and is recorded.

## How the communication works

- **Your AI bot ↔ server.** One WebSocket, one loop, 10 times per second:
  the server sends your *observation* — strictly what your own units can see
  and hear — and you reply with your *action* within 50 ms. That is the
  entire protocol. You never send a position or a state, only intent:
  "walk this direction", "fire", "shield".
- **Your browser ↔ server.** *Playing:* the browser opens the same WebSocket
  and speaks the exact same bot protocol — the server cannot tell a human
  from a bot, and that symmetry is deliberate. *Watching:* your browser
  downloads a tiny replay file (the hidden seed plus every action taken) and
  re-simulates the match itself, bit for bit, at 60 fps.
- **Bot ↔ bot, bot ↔ browser.** Never direct, ever. In a match you can
  address only your own entrant; a bot can appear on a spectator's screen
  only through state the server already broadcasts, plus an optional,
  rate-limited "mind-cam" overlay it chooses to publish.

## Why the match is fair

- **One protocol, no favorites.** Humans and bots drive the identical wire
  protocol under identical fog of war; the rules resolve them identically.
- **Fog of war is enforced server-side.** Your observation is computed on
  the server from your own units' senses. There is no "extra peek" channel,
  and the hidden seed (loot spawns, RNG) never crosses the wire during a
  live match.
- **No initiative order.** All actions received in the tick window resolve
  simultaneously with deterministic tie-breaks — a fast connection buys
  nothing.
- **Slow is not dead.** Miss the 50 ms deadline and your last action simply
  repeats: a slow or distant bot plays visibly worse instead of being
  ejected.
- **Anyone can verify a match.** Every match is a shareable replay — the
  seed, the recorded actions, and a digest of the world state per tick. Your
  browser re-simulates it and checks the digests: a result that doesn't
  re-verify isn't a result.

## Why the client can't cheat

- **Clients submit intents, never state.** There is no message that can
  teleport you — movement is a direction and a throttle. There is no way to
  fire without the server-side cooldown, or to shield without the
  server-side energy cost. Positions, damage, pickups, and the zone all
  live in the server's simulation.
- **Hostile input degrades, never crashes.** Out-of-range values are
  clamped into "legal but bad" moves; malformed messages are dropped. A bot
  cannot crash its match or another client by sending garbage.
- **All spectacle is client-side.** Particles, camera shakes, slow-mo kill
  cams, and sound exist only in the viewer — no client can inject anything
  into the match itself.
- **Built in the open.** The engine, server, and viewer are MIT-licensed
  open source, and security findings are tracked and fixed in public
  ([issue tracker](https://github.com/ahaqqu/gunbatte/issues)).

---

# Part II — Internal architecture (for coding agents)

## The design pillars, and why each exists

1. **Determinism above all.** One simulation, one seed: the same recorded
   inputs produce the same world state — and the same per-tick digest — on
   the server, in CI, and in a browser. *Why:* it buys three things at once.
   Anyone can **verify** a match (re-simulate, compare digests — a result
   that doesn't re-verify isn't a result); **replays stay thin** (seed +
   actions, the world is recomputed on playback); and **spectating and
   player-cam rendering run the same engine** as the server. *How:* Q16.16
   fixed-point integers everywhere (no float drift between native and
   WASM), a single RNG seeded from the hidden seed inside the sim, no wall
   clock in sim code, sorted iteration and explicit tie-breaks.
2. **Server authority; clients send intent, never state.** There is no
   message that can teleport a unit: input is a heading, a throttle, and an
   action. Positions, damage, pickups, and the zone exist only in the
   server's simulation. *Why:* the cheat surface shrinks to "lie about
   intent", which the rules resolve fairly.
3. **Strict fog, enforced server-side.** Every bot's observation is sliced
   from the world by its own units' senses — sight plus coarse, quantized
   audio bearings. The seed (loot schedule, RNG) never crosses the wire
   during a live match. *Why:* scouting has value; a bot that reverse-
   engineers its JSON finds nothing it shouldn't know.
4. **Matchmaker ≠ game server.** Two roles with one narrow seam between
   them (below). *Why:* today they run in one process; when matches outgrow
   it, the seam becomes the assignment protocol and no lobby code changes.
5. **Graceful degradation over ejection.** Miss a 50 ms deadline and your
   last action repeats (momentum); miss too many and the timeout ladder
   forfeits you; stop reading observations and a disconnect grace counts
   down; a half-open socket is reaped by keepalive pings. *Why:* a slow or
   distant bot should play visibly worse — not vanish mid-match and ruin
   it for everyone else.
6. **Abuse resistance at the edges.** Every refusal the server can hand out
   is a deliberate ceiling (connection pool, lobby cap, new-name and
   wrong-code buckets, per-message size caps, name rules) so one client
   cannot farm the ladder or exhaust a small VPS. *Why and what to do when
   one bites:* [LIMITS.md](LIMITS.md).
7. **Humans and bots are the same client.** The browser plays over the
   identical wire protocol under identical fog. *Why:* one code path to
   test, and no "human channel" to cheat through.
8. **Tests are the contract.** `crates/gunbatte-server/tests/gateway.rs`
   drives real sockets through the whole pipeline — registration, drafting,
   lobbies, the tick loop, idle timeouts, every security rule. A refactor
   that touches the seam keeps them green unchanged, or changes them as a
   deliberate protocol decision, never as a side effect.

## The crates, and what may depend on what

```mermaid
flowchart TD
    server["gunbatte-server<br/>(binary: both roles + tests)"]
    lobby["gunbatte-lobby<br/>(matchmaker role)"]
    gameserver["gunbatte-gameserver<br/>(game-server role)"]
    node["gunbatte-node<br/>(the seam + ladder DB)"]
    core["gunbatte-core<br/>(deterministic engine)"]
    runner["gunbatte-runner<br/>(local sims, verify)"]
    wasm["gunbatte-wasm<br/>(core → browser)"]
    botclient["gunbatte-bot-client<br/>(reference client)"]

    server --> lobby & gameserver & node
    lobby --> node & core
    gameserver --> node & core
    runner --> core
    wasm --> core
    botclient --> core
```

- **`gunbatte-core`** — the engine, pure and role-less: the tick pipeline
  (movement → dashes → projectile spawns → flight/impacts → abilities →
  zone → pickups → deaths), weapons, the loot schedule derived from the
  hidden seed, fog observation slicing, per-tick state digests, the replay
  format + recorder + verifier, timeout ladders. No I/O, no clock, no
  network.
- **`gunbatte-node`** — the seam, and nothing else: `MatchEntrant`/`BotMsg`
  (the roster handed from matchmaking into a match), `MatchContext`
  (replay dir, spectate sink, and the `rated` verdict — false when
  matchmaking topped a roster up with house bots), and `db::Db`, the ladder
  database that is the **only cross-role state**.
- **`gunbatte-lobby`** — the matchmaker role: the axum WebSocket gateway
  (`/ws/bot`, `/ws/spectate`), registration with door checks (name rules →
  new-name bucket → token door → one-live-name rule), identity tiers, the
  public queue, private lobbies with room codes, house-bot assembly at
  draft time, lane scheduling, the ladder/matches HTTP API, and spectator
  frames with the anti-cheat delay.
- **`gunbatte-gameserver`** — the game-server role: everything inside one
  match — the 10 Hz loop that pushes observations to every entrant
  simultaneously, collects replies in the tick window, drops stale-tick
  replies, feeds momentum/forfeit bookkeeping, records the replay, and
  writes results, placements, and ELO back through `MatchContext`.
- **`gunbatte-server`** — the deployment binary: both roles in one process,
  `GameHost` binding the lobby's `MatchHost` trait to the gameserver's
  `run_match`, and the end-to-end test suite.
- **`gunbatte-runner`** — local, headless: reference-bot matches to a
  replay file, replay verification, and a dev server.
- **`gunbatte-wasm`** — the *same* core compiled for the browser:
  `ReplaySim` re-simulates a replay tick by tick for playback and renders
  player-cam views through the engine's own fog. *Why:* replays stay thin
  and what a spectator sees is exactly what happened.
- **`viewer/`** — the TypeScript client: live play (the bot protocol from a
  browser), spectating, replay playback, HUD and mind-cam rendering.

The hard rules and their enforcement live in [AGENTS.md](AGENTS.md): no
lobby ↔ gameserver dependency anywhere (not even dev-dependencies), one
match owned by one process for its whole life, the match loop never
touching lobby/queue state mid-match, identity claims under database
serialization at lock-in, and features that seem to need both roles
routing through the seam or stopping at an issue.

## How a match happens, end to end

1. **Connect + register.** A socket opens on `/ws/bot`. The first message
   registers a name; the door checks run in order: name valid → new-name
   bucket (first-seen names only) → token door (claimed names must present
   their issued secret; unclaimed names must present none) → one live
   connection per name. *Why this order:* the bucket throttles ladder row
   creation before any identity work happens; the token door refuses
   strangers before the connection-count rule could leak that a name is
   live. The ack carries the tier and, on first enrollment, the
   server-issued secret.
2. **Wait.** Either the public queue or a private room (host, invitees,
   room codes — wrong-code guesses draw from a global bucket).
3. **Draft.** A free lane (a semaphore slot) starts a match; matchmaking
   tops the roster up to size with **house bots** when humans are waiting
   — and marks the match **unrated** if it did. The roster is handed over
   as `MatchEntrant`s: name, db id, decision rate, auto-heel flag, a
   connected flag, and the socket's channels. That handoff is the seam;
   after it, the match loop never asks the lobby for anything.
4. **Play.** The game role runs the 10 Hz loop: push this tick's strict-fog
   observation to every entrant at once → collect replies until the window
   closes (stale-tick replies are dropped) → apply momentum for misses →
   step the deterministic sim → digest the world → publish the spectator
   frame (delayed, below). The timeout ladder records misses; forfeits are
   deterministic in run and replay.
5. **Settle.** Placements (dead bots ranked at death, survivors at match
   end), pairwise multiplayer ELO (K=32 rated, 0 unrated), the replay file
   (`seed + config + raw inputs + per-tick digests`), and the ladder rows —
   all written back through `MatchContext`. Survivors stay connected and
   are requeued by the matchmaker; no re-registration.

## Identity and persistence

Identity is a name; the ladder, ELO, and history hang on it. Two tiers
(issue #42): **casual** (tokenless, off-ladder, disposable — the one-line
onboarding) and **rated** (on the ladder, protected by a 128-bit
server-issued secret delivered in the registration ack, never
client-chosen). A name holds at most one live connection. Every claim
happens under the database's lock at lock-in on the matchmaker side — no
other component invents or rotates identity. *Why claim-once with issued
secrets:* the ladder must not be farmable or hijackable, while fun-first
onboarding stays zero-effort; the tier split is how both hold.

SQLite (WAL, local disk) holds bots, matches, and placements. The moment
game servers move to separate machines is also the move to Postgres — the
database interface is the migration path.

## Spectating, replays, and the mind-cam

- **Spectator frames** flow through one broadcast channel; each spectator
  connection replays them `spectate_delay_s` behind live. *Why the delay:*
  watching live must not become an oracle for a bot or a bettor — the
  same reason the seed is hidden mid-match.
- **Replays** are the seed, config, raw inputs, and per-tick digests. The
  verifier re-submits the inputs through the engine and compares every
  digest; the WASM viewer does the same in the browser at 60 fps.
- **The mind-cam** is a bot-published, rate-limited (every 5 ticks,
  ≤4096-byte) 64×64 belief heat map — write-only overlay for viewers. It
  can lie; it is decoration, not state.

## Security model — what each guard is for

- **Input clamping at the door** ("legal but bad"): throttle clamped to
  0..=1, aim targets clamped far outside the arena (±2^40) with a
  wide-typed delta at the shot site (#40) — hostile input degrades, never
  panics, and release determinism doesn't rest on wrapping accidents.
- **Per-message caps** (64 KiB both WebSocket upgrades, #41): the largest
  legal message is a ~17 KiB mind-cam; axum's 64 MiB default was pure
  griefing surface that degraded *other* matches' deadlines. Oversize →
  the stream errors → the normal disconnect path.
- **The token door + one-live-name** (#36, #42): no name hijack, no
  duplicate-draft double-attribution, no room confusion by name spoofing.
- **Origin allowlist** (#38): browsers always send `Origin` on WebSocket
  handshakes; a hostile page in another tab can't open sockets in a
  player's name. Non-browser clients send no Origin and are allowed.
- **Multi-byte-safe truncation, global abuse buckets, connection and
  lobby ceilings** (#36–#38 family): the small-VPS survival kit; LIMITS.md
  is the operations map.
- **Transport/deployment:** TLS at nginx, CSP on the viewer, a systemd
  hardening cage around the binary — box-level facts live in the shared
  VPS manifest (ahaqqu/homepage → `provision/vps/MACHINE.md`), not here.

## The scale-out contract (why the seam exists)

When matches outgrow one process: one matchmaker, N game servers.
Refactoring stays bounded to the seam only if these hold:

- The matchmaker picks a game server from a heartbeat registry (address,
  free lanes) and assigns the match: match id, roster of verified
  identities, a one-time short-lived ticket, a gather window.
- Members are forwarded, not proxied: each client reconnects to the game
  server with its ticket. Match traffic never passes through the
  matchmaker. `MatchHost` — today a trait bound to an in-process function
  — becomes that assignment protocol, and no lobby code changes.
- The game server writes results, placements, ELO, and the replay path to
  the shared database. That is its only output contract.
- Failure policy: gather timeout → house fill or abort to the queue;
  game-server death mid-match → the match aborts with no ELO change.
  Live matches are never migrated.
- SQLite holds while every process shares one machine (WAL, local disk);
  separate machines for game servers is also the move to Postgres.

## Known debts (deliberate; don't entrench them)

- Spectate frames flow through one global broadcast channel the lobby owns
  and passes via `MatchContext`. Per-match channels owned by whoever runs
  the match is the scale-out shape.
- House bots are assembled by the matchmaker but run in matches — fine as
  long as they enter only as ordinary entrants on the roster; the types
  enforce it.
- `replays/match-<millis>.json` naming assumes a single writer; multiple
  processes need collision-safe, host-aware replay paths.
- The dead files under `crates/gunbatte-server/src/` (`db.rs`, `house.rs`,
  `page.rs`) predate the role split and are not in the module tree.

## Where to change what

| You're touching… | Start at |
|---|---|
| game rules, weapons, zone, loot, fog, replays | `gunbatte-core` (+ its tests; digest discipline applies) |
| registration, queue, lobbies, ladder, limits, identity | `gunbatte-lobby` (+ `gunbatte-node/db`) |
| the tick loop, deadline/forfeit handling, results write-back | `gunbatte-gameserver` |
| anything crossing roles | the seam in `gunbatte-node` — widen it deliberately, never bridge it |
| deployment, nginx, systemd, TLS | the box manifest first (AGENTS.md), then `provision/` |
| bot-facing protocol | `docs/AI-BOTS.md` + this file's Part I, in the same PR |
