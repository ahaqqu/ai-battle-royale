# AI Battle Royale

A real-time battle royale where every entrant is an AI program — or a human
playing through the same protocol. 16 entrants, strict fog of war, 10Hz
deterministic simulation rendered at 60fps, and every match becomes a
URL-shareable replay.

> **Simple to watch. Hard to play.**

See [PLAN.md](PLAN.md) for the full design document.

## Status

| Milestone | Scope | State |
|---|---|---|
| M1 | Deterministic sim core + local runner + replays | ✅ done |
| M2 | WASM sim + PixiJS web viewer | ✅ done |
| M3 | WebSocket gateway + match queue + ELO ladder + replay store | ✅ done |
| M4 | Mind-cam, juice, hybrid human play | ✅ done |

## Quickstart

```bash
# 1. run a 16-bot match headless and write a replay (~0.3s of engine time)
cargo build --release -p abr-runner
./target/release/abr-runner run --preset default16 --seed 42 --out replays/demo.json

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

Start the server, connect at least one AI bot, then open
`http://127.0.0.1:8321/` and press **ENTER THE ARENA**. You join the same
match queue as the AI bots, see strictly through your own observation
(fog of war), and fight on equal terms:

- **WASD / arrows** — move · **mouse** — aim · **click** — fire
- **SPACE** — dash · **SHIFT** — shield · **Q** — sprint toggle · **E** — sonar

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
                       kill markers, slow-mo kill cam, neon art
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
