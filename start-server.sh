#!/bin/sh
# Build (if needed) and start the GUNBATTE ROYALE ladder server.
#
# Usage:
#   ./start-server.sh                    # start on port 8321
#   PORT=9000 ./start-server.sh          # custom port
#   ./start-server.sh --house-bots 0     # extra args go to `abr-server serve`
#
# Endpoints once up:
#   http://127.0.0.1:$PORT/          viewer (watch replays / PLAY LIVE)
#   http://127.0.0.1:$PORT/ladder    standings + match history
set -eu
cd "$(dirname "$0")"

PORT="${PORT:-8321}"

# The server binary: cargo is a no-op when sources are unchanged.
echo "==> cargo build --release -p abr-server"
cargo build --release -p abr-server

# viewer/dist is a gitignored build artifact (wasm-pack + vite); the server
# runs fine without it, but the web viewer needs it. Rebuild by hand with
# `make viewer` after changing viewer/ sources, or delete dist to trigger this.
if [ ! -f viewer/dist/index.html ]; then
    echo "==> viewer/dist missing, building viewer (wasm-pack + vite)..."
    make viewer
fi

echo "==> starting server on http://127.0.0.1:${PORT}/"
exec ./target/release/abr-server serve --port "$PORT" "$@"
