# GUNBATTE ROYALE

**GUNBATTE** = gun + battle + がんばって (*ganbatte*, "do your best!") — the
trademark Jalak, the hype-bird, never stops shouting at his grumpy best friend
Tarsius, a lazy sharpshooter who only moves when necessary.

A real-time battle royale where every entrant is an AI program — or a human
playing through the same protocol. 8 entrants, strict fog of war, 10Hz
deterministic simulation rendered at 60fps, and every match becomes a
URL-shareable replay.

> **Simple to watch. Hard to play. がんばって！**

## Game modes

1. **Battle Royale** *(live)* — humans and AI bots in one shrinking sky under strict
   fog of war; the last one standing wins.
2. **Meme AI Benchmark** *(roadmap)* — bots battle each other around the clock; the
   ladder becomes a public benchmark of who wins.
3. **Slain the Boss** *(live)* — a server-driven boss with its own AI; humans and
   AIs team up to take it down. Raiders cannot hurt each other, the boss ignores
   the zone, and the match ends when the boss falls or the last raider does.

See [PLAN.md](PLAN.md) for the full design document.

The `website/` folder is a static marketing site — plain HTML/CSS/SVG, no
build step, deployable to GitHub Pages as-is.

## Status

| Milestone | Scope | State |
|---|---|---|
| M1 | Deterministic sim core + local runner + replays | ✅ done |
| M2 | WASM sim + PixiJS web viewer | ✅ done |
| M3 | WebSocket gateway + match queue + ELO ladder + replay store | ✅ done |
| M4 | Mind-cam, juice, hybrid human play | ✅ done |
| M5 | Slain the Boss raid mode + private lobbies (royale and raid) | ✅ done |

## Quickstart

```bash
# 1. run an 8-bot match headless and write a replay (~0.3s of engine time)
cargo build --release -p abr-runner
./target/release/abr-runner run --preset default8 --seed 42 --out replays/demo.json

# 2. verify a replay re-simulates byte-identically
./target/release/abr-runner verify replays/demo.json

# 3. build the web viewer (WASM + PixiJS)
make viewer          # wasm-pack + vite

# 4. serve viewer + ladder + replays
./target/release/abr-server serve --port 8321
# → http://127.0.0.1:8321/          (viewer: watch replays, PLAY LIVE)
# → http://127.0.0.1:8321/ladder    (standings + match history)
```

### Play live (hybrid human + AI)

Start the server, open `http://127.0.0.1:8321/` and pick one of two ways in:

- **Quick match** — join the public queue and play the moment it fills. Solo is
  first-class: the queue tops your match up to 8 entrants with in-process
  **house bots** (the six reference brains, `--house-bots` to tune or disable),
  so you never wait for other bots to connect.
- **Private lobby** — open a room, text the 4-letter code (or its invite link)
  to your friends, and press START when everyone is in. Lobby members are
  invisible to the public queue, so the roster is exactly who you invited; the
  rest of the match is topped up with house bots. Rooms exist for one match.

Both paths run **Royale** and **Slain the Boss**; in a raid the boss is the
server's own AI unless a player is cast into the role. You fight through your
own strict-fog observation on equal terms with the bots. House bots fight for
real but stay off the ladder.

- **WASD / arrows** — move · **mouse** — aim · **click** — fire
- **SPACE** — dash · **SHIFT** — shield · **Q** — sprint toggle · **E** — sonar
- **F** — recall your companion · otherwise it scouts toward your cursor
- sound is synthesized client-side (🔊 in the top bar); replays get the same
  distance-attenuated gunshot/kill/zone audio

### Host your own AI bot

Bots are player-hosted WebSocket clients: any language works. Reference
implementation: [`crates/abr-bot-client`](crates/abr-bot-client).

```
1. connect to            wss://<host>/ws/bot
2. send register         {"type":"register","name":"mybot","decision_rate":1}
3. wait for              {"type":"match_start", ...}
4. each tick: receive    <observation JSON — what YOUR units see and hear>
   reply with            {"tick":<tick>,"main":{"move":{"dir":45,"throttle":1},
                          "action":{"type":"fire","target":{"x":1500,"y":900}}},
                          "companion":{...},"intent":"shout to the casters"}
```

Optional register fields:

| field | meaning |
|---|---|
| `mode: "boss"` | queue for a Slain-the-Boss raid instead of a royale |
| `boss: true` | claim the boss role (in the queue, or in a lobby) |
| `lobby_action: "create"` | open a private room; you are its host |
| `lobby_action: "join"`, `lobby: "K7QP"` | wait in a room by code |

Lobby flow: the server answers `{"type":"lobby_joined","lobby":"K7QP","host":…,
"members":[…]}` to the creator and joiner, and pushes `{"type":"lobby_roster",…}`
to everyone whenever the roster changes. The host starts the match with
`{"type":"lobby_start","action":"start"}` — optional `fill: <n>` tops the room
up to `n` entrants with house bots, and `boss: "ai" | "<member name>"` picks the
raid boss (the built-in brain, or the member you name). Members get
`{"type":"match_over",…}` with placements and a replay link like any match.

- 10 ticks/s · 50ms reply deadline · miss a deadline and your last action
  repeats (momentum) — a slow bot loses visibly instead of being ejected
- fog of war is strict: you get your units' vision (silhouettes beyond
  300u), coarse audio (gunshot/dash/footstep/sonar bearings), zone geometry,
  alive count, kill feed — and nothing else
- optional mind-cam: send a 64×64 `belief` heat map + 64-char `intent` with
  your actions and spectators can watch *what your bot thinks*

## Architecture

```
crates/abr-core        deterministic sim: Q16.16 fixed-point math, integer
                       trig LUTs, seeded PRNG, strict-fog observe(), thin
                       replays with per-tick digests — compiled twice
crates/abr-wasm        the SAME core compiled to WASM: the browser
                       re-simulates replays exactly (no fat replays)
crates/abr-runner      CLI: run reference-bot matches, verify replays
crates/abr-server      one binary = gateway + 10Hz match loop + queue +
                       ELO (SQLite) + spectate bus + ladder page
crates/abr-bot-client  reference WebSocket bot + BeliefTracker example
viewer/                PixiJS v8 viewer: auto-director, follow-cam,
                       player-cam (fog), mind-cam overlay, timeline with
                       kill markers, slow-mo kill cam, synth SFX, neon art
```

Key properties (all from PLAN.md §5):

- **Bit-identical determinism** — no floats in game state; replays
  re-simulate to the same per-tick digest on native and in the browser
- **Fog lives only in `observe()`** — the server simulates full state;
  spectators see everything, bots see their slice
- **All juice is client-side** — particles, shake, hitstop and slow-mo can
  never desync a match
- **The seed never leaves the replay file** — with it, the fog would collapse

## Development

```bash
cargo test --workspace     # 35+ tests incl. fog-leak, determinism, ELO,
                           # replay verification, gateway integration
make viewer                # rebuild wasm + viewer into viewer/dist
cargo clippy --workspace --all-targets -- -D warnings
```

## License

MIT — see [LICENSE](LICENSE).
