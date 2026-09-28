#!/usr/bin/env bash
# provision/vps/deploy.sh — build locally, ship artifacts, restart the service.
#
# First run (installs unit + nginx + TLS; sudo prompts over ssh):
#   GUNBATTE_GAME_HOST=play.example.com GUNBATTE_SITE_HOST=gunbatte.example.com \
#   GUNBATTE_EMAIL=you@example.com ./provision/vps/deploy.sh --bootstrap
#
# Every later update (no sudo, no prompts):
#   GUNBATTE_GAME_HOST=play.example.com ./provision/vps/deploy.sh
#
# The game hostname is required on every run: it is substituted into the
# website's PLAY NOW / LADDER links at upload time, so the repo copy stays
# generic (same placeholder convention as the kajianq provisioning).
set -euo pipefail

GUNBATTE_SSH="${GUNBATTE_SSH:-ahaqqu@62.83.35.220}"
GUNBATTE_DIR="${GUNBATTE_DIR:-/home/ahaqqu/gunbatte}"
GUNBATTE_PORT="${GUNBATTE_PORT:-8321}"
GUNBATTE_USER="${GUNBATTE_USER:-ahaqqu}"
: "${GUNBATTE_GAME_HOST:?set GUNBATTE_GAME_HOST (game hostname, e.g. play.example.com)}"

BOOTSTRAP=0
[ "${1:-}" = "--bootstrap" ] && BOOTSTRAP=1

repo="$(cd "$(dirname "$0")/../.." && pwd)"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

echo "▶ building viewer (wasm-pack + vite)…"
( cd "$repo" && make viewer )

echo "▶ building abr-server (release)…"
( cd "$repo" && cargo build --release -p abr-server )

# Stage the upload. The website copy gets its CTAs pointed at the real game
# host; the repo copy keeps the localhost default for local dev.
cp "$repo/target/release/abr-server" "$stage/"
cp -r "$repo/viewer/dist" "$stage/viewer-dist"
mkdir -p "$stage/website"
sed "s|http://127.0.0.1:8321|https://$GUNBATTE_GAME_HOST|g" \
    "$repo/website/index.html" > "$stage/website/index.html"
cp -r "$repo/website/assets" "$repo/website/style.css" "$stage/website/"
cp -r "$repo/provision" "$stage/provision"

echo "▶ uploading to $GUNBATTE_SSH:$GUNBATTE_DIR …"
ssh "$GUNBATTE_SSH" "mkdir -p '$GUNBATTE_DIR'"
rsync -a --delete "$stage/viewer-dist/" "$GUNBATTE_SSH:$GUNBATTE_DIR/viewer/dist/"
rsync -a --delete "$stage/website/"     "$GUNBATTE_SSH:$GUNBATTE_DIR/website/"
rsync -a --delete "$stage/provision/"   "$GUNBATTE_SSH:$GUNBATTE_DIR/provision/"
rsync -a "$stage/abr-server"            "$GUNBATTE_SSH:$GUNBATTE_DIR/abr-server"
# ladder.db and replays/ live in GUNBATTE_DIR too and are deliberately NOT
# synced — they are the server's state.

if [ "$BOOTSTRAP" = 1 ]; then
    : "${GUNBATTE_SITE_HOST:?--bootstrap needs GUNBATTE_SITE_HOST (website hostname)}"
    : "${GUNBATTE_EMAIL:?--bootstrap needs GUNBATTE_EMAIL (for the certbot account)}"
    echo "▶ bootstrapping VPS (sudo password may be prompted once)…"
    remote_cmd="cd $GUNBATTE_DIR/provision/vps && sudo -v && GUNBATTE_GAME_HOST=$GUNBATTE_GAME_HOST GUNBATTE_SITE_HOST=$GUNBATTE_SITE_HOST GUNBATTE_EMAIL=$GUNBATTE_EMAIL GUNBATTE_DIR=$GUNBATTE_DIR GUNBATTE_PORT=$GUNBATTE_PORT GUNBATTE_USER=$GUNBATTE_USER ./apply.sh"
    ssh -t "$GUNBATTE_SSH" "$remote_cmd"
else
    if ssh "$GUNBATTE_SSH" "systemctl list-unit-files gunbatte.service --no-legend" | grep -q gunbatte; then
        echo "▶ restarting gunbatte.service (passwordless via sudoers drop-in)…"
        ssh "$GUNBATTE_SSH" "sudo -n systemctl restart gunbatte.service && systemctl is-active gunbatte.service"
    else
        echo "⚠ gunbatte.service is not installed on the VPS yet — artifacts uploaded."
        echo "  Finish setup once with: ./provision/vps/deploy.sh --bootstrap"
    fi
fi

echo "✔ deployed."
