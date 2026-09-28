#!/usr/bin/env bash
# provision/vps/deploy.sh — build locally, ship artifacts, restart the service.
#
# First run (installs identity + unit + nginx + TLS; sudo prompts over ssh):
#   GUNBATTE_GAME_HOST=play.example.com GUNBATTE_SITE_HOST=site.example.com \
#   GUNBATTE_EMAIL=you@example.com ./provision/vps/deploy.sh --bootstrap
#
# Every later update (no sudo, no prompts):
#   ./provision/vps/deploy.sh
#
# Identity model (same as the kajianq provisioning): CI and regular deploys
# authenticate as the restricted $GUNBATTE_DEPLOY_USER (default
# kajianq-deploy — override with the VPS_USER Actions variable), never as
# the admin login. --bootstrap is the one exception: it runs as the admin
# account because it creates the deploy identity itself, staging the
# artifacts in the admin's home and letting sudo move them into place.
#
# The game hostname is substituted into the website's PLAY NOW / LADDER links
# at upload time, so the repo copy stays generic.
set -euo pipefail

# All configuration comes from the environment or, when unset, from the
# repository's GitHub Actions variables — the same source CI reads, so there
# is exactly one place where deployment values live. Nothing is hardcoded.
fetch() {
    local name="$1" val=""
    # eval instead of ${!name:-} — modifiers are not allowed inside bash's
    # indirect expansion (it would read the variable name as "name:").
    eval "val=\"\${$name:-}\""
    if [ -n "$val" ]; then printf '%s\n' "$val"; return 0; fi
    if command -v gh >/dev/null 2>&1; then
        val="$(gh variable get "$name" 2>/dev/null || true)"
        if [ -n "$val" ]; then printf '%s\n' "$val"; return 0; fi
    fi
    echo "!! $name is not set — export it, or add it as a GitHub repository variable (Settings → Secrets and variables → Actions → Variables)" >&2
    return 1
}
GUNBATTE_DEPLOY_USER="$(fetch VPS_USER)"
VPS_HOST="$(fetch VPS_HOST)"
GUNBATTE_GAME_HOST="$(fetch GUNBATTE_GAME_HOST)"
GUNBATTE_PORT="$(fetch GUNBATTE_PORT)"
GUNBATTE_SSH="${GUNBATTE_SSH:-$GUNBATTE_DEPLOY_USER@$VPS_HOST}"
GUNBATTE_DIR="${GUNBATTE_DIR:-/home/$GUNBATTE_DEPLOY_USER/gunbatte}"

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

if [ "$BOOTSTRAP" = 1 ]; then
    GUNBATTE_SITE_HOST="$(fetch GUNBATTE_SITE_HOST)"
    GUNBATTE_EMAIL="$(fetch GUNBATTE_EMAIL)"
    # --bootstrap is run by a human and is the ONLY path that touches the
    # admin account. CI never sees an admin identity — so the admin SSH
    # target is never stored in GitHub config; you name it here, each time.
    : "${GUNBATTE_ADMIN_SSH:?--bootstrap runs as the admin, which CI never does — export GUNBATTE_ADMIN_SSH=admin-user@vps-host for this one command}"
    pub_file="${GUNBATTE_DEPLOY_PUBKEY_FILE:-$HOME/.ssh/gunbatte-deploy.pub}"
    if [ ! -f "$pub_file" ]; then
        echo "!! CI public key not found at $pub_file" >&2
        echo "   create the keypair first:  ssh-keygen -t ed25519 -f ~/.ssh/gunbatte-deploy -N ''" >&2
        exit 1
    fi

    # 1. stage on the VPS in the ADMIN's home (no sudo needed to write there).
    staging="/home/$(echo "$GUNBATTE_ADMIN_SSH" | cut -d@ -f1)/gunbatte-staging"
    echo "▶ staging upload to $GUNBATTE_ADMIN_SSH:$staging …"
    # pre-create the nested parents — rsync only makes the last path component
    ssh "$GUNBATTE_ADMIN_SSH" "mkdir -p '$staging/viewer/dist' '$staging/website' '$staging/provision/vps'"
    rsync -a --delete "$stage/viewer-dist/" "$GUNBATTE_ADMIN_SSH:$staging/viewer/dist/"
    rsync -a --delete "$stage/website/"     "$GUNBATTE_ADMIN_SSH:$staging/website/"
    rsync -a --delete "$stage/provision/"   "$GUNBATTE_ADMIN_SSH:$staging/provision/"
    rsync -a "$stage/abr-server"            "$GUNBATTE_ADMIN_SSH:$staging/abr-server"

    # 2. one interactive sudo session: move everything into place, then run
    #    apply.sh AS ROOT — the app dir belongs to the deploy identity (0700
    #    home), so the invoking user could not even cd into it afterwards.
    #    apply.sh's own sudo calls are no-ops when already root.
    echo "▶ bootstrapping VPS as $GUNBATTE_ADMIN_SSH (sudo password may be prompted once)…"
    remote_cmd="sudo -v && sudo rsync -a $staging/ $GUNBATTE_DIR/ && sudo env GUNBATTE_GAME_HOST='$GUNBATTE_GAME_HOST' GUNBATTE_SITE_HOST='$GUNBATTE_SITE_HOST' GUNBATTE_EMAIL='$GUNBATTE_EMAIL' GUNBATTE_DIR='$GUNBATTE_DIR' GUNBATTE_PORT='$GUNBATTE_PORT' GUNBATTE_DEPLOY_USER='$GUNBATTE_DEPLOY_USER' bash $GUNBATTE_DIR/provision/vps/apply.sh"
    ssh -t "$GUNBATTE_ADMIN_SSH" "$remote_cmd"
else
    echo "▶ uploading to $GUNBATTE_SSH:$GUNBATTE_DIR …"
    # pre-create the nested parents — rsync only makes the last path component
    ssh "$GUNBATTE_SSH" "mkdir -p '$GUNBATTE_DIR/viewer/dist' '$GUNBATTE_DIR/website' '$GUNBATTE_DIR/provision/vps'"
    rsync -a --delete "$stage/viewer-dist/" "$GUNBATTE_SSH:$GUNBATTE_DIR/viewer/dist/"
    rsync -a --delete "$stage/website/"     "$GUNBATTE_SSH:$GUNBATTE_DIR/website/"
    rsync -a --delete "$stage/provision/"   "$GUNBATTE_SSH:$GUNBATTE_DIR/provision/"
    rsync -a "$stage/abr-server"            "$GUNBATTE_SSH:$GUNBATTE_DIR/abr-server"
    # ladder.db and replays/ live in GUNBATTE_DIR too and are deliberately NOT
    # synced — they are the server's state.
    if ssh "$GUNBATTE_SSH" "systemctl list-unit-files gunbatte.service --no-legend" | grep -q gunbatte; then
        echo "▶ restarting gunbatte.service (passwordless via sudoers drop-in)…"
        ssh "$GUNBATTE_SSH" "sudo -n systemctl restart gunbatte.service && systemctl is-active gunbatte.service"
    else
        echo "⚠ gunbatte.service is not installed on the VPS yet — artifacts uploaded."
        echo "  Finish setup once with: ./provision/vps/deploy.sh --bootstrap"
    fi
fi

echo "✔ deployed."
