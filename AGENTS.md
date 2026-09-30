# AGENTS.md — working rules for coding agents

Engineering conventions for anyone (human or agent) changing this repo. The
public-facing overview lives in [ARCHITECTURE.md](ARCHITECTURE.md); this file
covers the internal boundary that keeps the project able to scale out.

## Commands

- `make test` — full workspace tests, then again in release (the gateway
  integration tests are timing-sensitive; both passes must be green).
- `make ci` — tests plus `cargo clippy --workspace --all-targets -- -D
  warnings`. Clippy with `-D warnings` is the merge gate; keep it clean.
- `make build` / `make viewer` — native sim; WASM viewer into `viewer/dist`.

## The one architectural rule: matchmaker ≠ game server

The project is heading toward a two-role topology: a **matchmaker** that owns
people (connections, rooms, the queue, the ladder) and **game servers** that
own matches. They are deployed together today and that stays supported — the
boundary is about code, not processes — but the two roles must never grow
into each other:

- **Matchmaker role** — WebSocket intake and registration, token/identity
  handling, the public queue, private lobbies and room codes, house-bot
  assembly at draft time, matchmaking and lane scheduling, ELO/standings,
  the ladder page, replay listing.
- **Game-server role** — everything inside one match: the 10 Hz loop, fog
  observations, the 50 ms reply deadline, stall/forfeit handling, replay
  recording, and the results/ELO write-back at match end.
- **Shared** — `crates/gunbatte-core` (engine, match config, wire types,
  replay format) and `crates/gunbatte-node` (the seam: the entrant handoff,
  per-match resources, and the ladder database). Both roles depend on the
  shared crates; the roles never depend on each other.

### Package layout (the split is physical)

- `crates/gunbatte-core` — the deterministic engine. Pure; no role owns it.
- `crates/gunbatte-node` — the seam: `MatchEntrant`/`BotMsg` (the roster
  handoff), `MatchContext` (per-match resources), and the ladder database.
  The database is the only cross-role state.
- `crates/gunbatte-lobby` — the matchmaker role.
- `crates/gunbatte-gameserver` — the game-server role.
- `crates/gunbatte-server` — the binary that runs both roles in one process,
  the `GameHost` binding between them, and the end-to-end tests.

Dependency graph: `gunbatte-server` → {lobby, gameserver, node};
lobby → {node, core}; gameserver → {node, core}. There is no
lobby ↔ gameserver edge anywhere, not even a dev-dependency.

### Hard dependency rules

1. **No cross-dependency, ever.** Matchmaker code may not reach into
   game-server code and vice versa. The only things that cross the seam are
   the roster handoff (entrants + config in) and the results/replay
   write-back (out).
2. **One match, one owner.** A running match belongs to exactly one process
   for its whole life. Its state never crosses a network, a store, or a
   lock shared with the matchmaker. Never add code that reads or writes a
   live match from outside its owner.
3. **The match loop does not touch the lobby or the queue mid-match.**
   Identity, ELO lookups, and queue state are resolved at draft time; the
   loop consumes an entrant view (name, rates, connected flag, channels)
   and nothing else. Post-match requeue of survivors is matchmaking work
   and lives in `Server::spawn_match`, after the host future resolves.
4. **The database is the identity arbiter.** Names and tokens are claimed
   once, under database serialization, on the matchmaker side at lock-in.
   No other component invents, rotates, or trusts identity.
5. **A feature that seems to need both roles** either routes through the
   narrow interface (roster in → results out) or stops and raises an issue.
   A change that widens the seam is preferable to one that bridges it.

### Where the seam sits

- The handoff: `BotHandle::entrant()` (lobby) builds a `MatchEntrant`
  (node); `Server::spawn_match` passes entrants + `MatchContext` (db, replay
  dir, spectate sink, `rated` verdict for house-filled matches) to
  `MatchHost::host_match`; `GameHost` (gunbatte-server) binds that to
  `run_match` (gameserver).
- `MatchHost` is the only way matchmaking reaches a match. When the roles
  split across processes, the trait's implementation becomes the assignment
  protocol and no lobby code changes.
- Lobby-side state on a handle (queue/lobby membership, mode, human flag)
  is matchmaking-private; game-side code consumes only the `MatchEntrant`.

### Known seam debts (future work; don't entrench them)

- Spectate frames flow through one global broadcast channel the lobby owns
  and passes via `MatchContext`. Per-match channels owned by whoever runs
  the match is the scale-out shape.
- House bots are assembled by the matchmaker (`house.rs`) but run in
  matches. That stays fine as long as they enter only as ordinary entrants
  on the roster — the types now enforce it.
- `replays/match-<millis>.json` naming assumes a single writer; multiple
  processes need collision-safe names and host-aware replay paths.

## The scale-out contract (why the rule exists)

When matches outgrow one process, the topology becomes: one matchmaker, N
game servers. Refactoring stays bounded to the seam only if these hold:

- The matchmaker picks a game server from a heartbeat registry (address,
  free lanes) and assigns the match: match id, roster of verified
  identities, a one-time short-lived ticket, a gather window.
- Members are forwarded, not proxied: each client reconnects to the game
  server with its ticket. Match traffic never passes through the matchmaker.
- The game server writes results, placements, ELO, and the replay path to
  the shared database. That is its only output contract.
- Failure policy: gather timeout → house fill (the existing mechanic) or
  abort back to the queue; game-server death mid-match → the match is
  aborted with no ELO change. Live matches are never migrated.
- Identity claims happen on the matchmaker at lock-in, via the database —
  this stays true with N game servers.
- SQLite holds while every process shares one machine (WAL, local disk);
  moving game servers to separate machines is also the move to Postgres.

## Tests are the contract

`crates/gunbatte-server/tests/gateway.rs` drives real sockets through the whole
pipeline — registration, drafting, lobbies, the tick loop, idle timeouts.
Any refactor that touches the seam must keep these green unchanged, or
change them as a deliberate protocol decision — never as a side effect.
