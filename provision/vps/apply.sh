#!/usr/bin/env bash
# provision/vps/apply.sh — one-time (and re-runnable) VPS bootstrap.
#
# Run ON the VPS, from the deployed app tree, with sudo credentials cached:
#   sudo -v && GUNBATTE_GAME_HOST=play.example.com GUNBATTE_SITE_HOST=gunbatte.example.com \
#     GUNBATTE_EMAIL=you@example.com ./apply.sh
#
# Normally you do not run this by hand: provision/vps/deploy.sh --bootstrap
# uploads the artifacts and invokes it over ssh for you.
#
# What it does, in order:
#   1. installs the systemd unit (dedicated user, loopback bind, own state dir)
#   2. installs a sudoers drop-in so future deploys can restart the unit
#      without a password (scoped to THIS service only)
#   3. installs the nginx site file — add-only, no other app's block is touched
#   4. runs certbot for both hostnames (skipped if DNS is not live yet)
#   5. (re)starts gunbatte.service
#
# Idempotent: safe to re-run. The nginx file is only installed once, so a
# re-run never clobbers certbot's TLS edits.
set -euo pipefail

: "${GUNBATTE_GAME_HOST:?set GUNBATTE_GAME_HOST (game hostname, e.g. play.example.com)}"
: "${GUNBATTE_SITE_HOST:?set GUNBATTE_SITE_HOST (website hostname, e.g. gunbatte.example.com)}"
GUNBATTE_DIR="${GUNBATTE_DIR:-$HOME/gunbatte}"
GUNBATTE_PORT="${GUNBATTE_PORT:-8321}"
GUNBATTE_USER="${GUNBATTE_USER:-$(id -un)}"

here="$(cd "$(dirname "$0")" && pwd)"

echo "▶ game host : $GUNBATTE_GAME_HOST"
echo "▶ site host : $GUNBATTE_SITE_HOST"
echo "▶ app dir   : $GUNBATTE_DIR (user: $GUNBATTE_USER, port: $GUNBATTE_PORT, bind: 127.0.0.1)"

# --- 1. systemd unit ---------------------------------------------------------
sed -e "s|__GUNBATTE_USER__|$GUNBATTE_USER|g" \
    -e "s|__GUNBATTE_DIR__|$GUNBATTE_DIR|g" \
    -e "s|__GUNBATTE_PORT__|$GUNBATTE_PORT|g" \
    "$here/gunbatte.service" | sudo tee /etc/systemd/system/gunbatte.service >/dev/null
sudo systemctl daemon-reload
sudo systemctl enable gunbatte.service
echo "✓ systemd: gunbatte.service installed + enabled"

# --- 2. sudoers drop-in: passwordless restart of THIS unit only --------------
sudoers_tmp="$(mktemp)"
printf '%s ALL=(root) NOPASSWD: /usr/bin/systemctl restart gunbatte.service, /usr/bin/systemctl status gunbatte.service\n' \
    "$GUNBATTE_USER" > "$sudoers_tmp"
if sudo visudo -c -q -f "$sudoers_tmp"; then
    sudo install -m 0440 "$sudoers_tmp" /etc/sudoers.d/gunbatte-deploy
    echo "✓ sudoers: $GUNBATTE_USER may restart/status gunbatte.service without a password"
else
    echo "!! visudo rejected the drop-in — skipping (deploys will need the sudo password)" >&2
fi
rm -f "$sudoers_tmp"

# --- 3. nginx site file (add-only) -------------------------------------------
if sudo test -f /etc/nginx/sites-enabled/gunbatte.conf; then
    echo "✓ nginx: gunbatte.conf already installed — leaving certbot's TLS edits alone"
else
    sed -e "s|__GUNBATTE_GAME_HOST__|$GUNBATTE_GAME_HOST|g" \
        -e "s|__GUNBATTE_SITE_HOST__|$GUNBATTE_SITE_HOST|g" \
        -e "s|__GUNBATTE_DIR__|$GUNBATTE_DIR|g" \
        -e "s|__GUNBATTE_PORT__|$GUNBATTE_PORT|g" \
        "$here/nginx/gunbatte.conf" | sudo tee /etc/nginx/sites-available/gunbatte.conf >/dev/null
    sudo ln -sf /etc/nginx/sites-available/gunbatte.conf /etc/nginx/sites-enabled/gunbatte.conf
    sudo nginx -t
    sudo systemctl reload nginx
    echo "✓ nginx: gunbatte site installed (HTTP only; certbot adds TLS next)"
fi

# --- 4. TLS via certbot (needs both hostnames resolving to THIS machine) ------
resolved="$(getent hosts "$GUNBATTE_GAME_HOST" | awk '{print $1; exit}')"
skip_certbot=0
if [ -z "$resolved" ]; then
    echo "!! DNS: $GUNBATTE_GAME_HOST does not resolve yet — skipping certbot (re-run apply.sh after DNS goes live)"
    skip_certbot=1
elif ! ip -o addr | grep -qF "$resolved"; then
    echo "!! DNS: $GUNBATTE_GAME_HOST resolves to $resolved, which is not this machine — skipping certbot"
    skip_certbot=1
elif sudo test -d "/etc/letsencrypt/live/$GUNBATTE_GAME_HOST"; then
    echo "✓ certbot: certificate for $GUNBATTE_GAME_HOST already exists"
else
    if [ -z "${GUNBATTE_EMAIL:-}" ]; then
        echo "!! GUNBATTE_EMAIL not set — skipping certbot (set it and re-run to get TLS)"
        skip_certbot=1
    else
        sudo certbot --nginx \
            -d "$GUNBATTE_GAME_HOST" -d "$GUNBATTE_SITE_HOST" \
            -m "$GUNBATTE_EMAIL" --agree-tos --non-interactive --redirect
    fi
fi

# --- 5. start / refresh the service -------------------------------------------
if [ -x "$GUNBATTE_DIR/abr-server" ]; then
    sudo systemctl restart gunbatte.service
    sleep 1
    systemctl is-active gunbatte.service
    echo "✓ gunbatte.service is up (state dir: $GUNBATTE_DIR — ladder.db + replays/ live here)"
else
    echo "!! binary $GUNBATTE_DIR/abr-server not found — upload artifacts first (deploy.sh does this)"
fi

if [ "$skip_certbot" = 1 ]; then
    echo "✔ bootstrap done (no TLS yet) — finish DNS, then re-run apply.sh"
else
    echo "✔ bootstrap done — https://$GUNBATTE_GAME_HOST  ·  https://$GUNBATTE_SITE_HOST"
fi
