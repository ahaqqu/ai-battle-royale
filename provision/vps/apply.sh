#!/usr/bin/env bash
# provision/vps/apply.sh — one-time (and re-runnable) VPS bootstrap.
#
# Run ON the VPS, from the deployed app tree, with sudo credentials cached:
#   sudo -v && GUNBATTE_GAME_HOST=play.example.com GUNBATTE_SITE_HOST=gunbatte.example.com \
#     GUNBATTE_EMAIL=you@example.com GUNBATTE_DEPLOY_PUBKEY="$(cat ~/.ssh/gunbatte-deploy.pub)" \
#     ./apply.sh
#
# Normally you do not run this by hand: provision/vps/deploy.sh --bootstrap
# uploads the artifacts and invokes it over ssh for you.
#
# The deploy identity is kajianq-deploy (the same restricted account the
# KajianQ provisioning uses — shared username, scoped per-app grants):
#   - this script creates the account if missing and appends the CI pubkey
#     to its authorized_keys (idempotent — kajianq's own key line is kept)
#   - the sudoers grant for gunbatte.service lives in its OWN file
#     (/etc/sudoers.d/gunbatte-deploy), so kajianq's grant file is untouched
#   - the gunbatte.service unit also runs as this user, so CI's rsync target
#     and the process are one identity — the admin login (ahaqqu) is never
#     a deploy credential
#
# Idempotent: safe to re-run. The nginx file is only installed once, so a
# re-run never clobbers certbot's TLS edits.
set -euo pipefail

: "${GUNBATTE_GAME_HOST:?set GUNBATTE_GAME_HOST (game hostname, e.g. play.example.com)}"
: "${GUNBATTE_SITE_HOST:?set GUNBATTE_SITE_HOST (website hostname, e.g. gunbatte.example.com)}"
: "${GUNBATTE_DEPLOY_PUBKEY:?set GUNBATTE_DEPLOY_PUBKEY (contents of the CI public key)}"
GUNBATTE_DEPLOY_USER="${GUNBATTE_DEPLOY_USER:-kajianq-deploy}"
GUNBATTE_DIR="${GUNBATTE_DIR:-/home/$GUNBATTE_DEPLOY_USER/gunbatte}"
GUNBATTE_PORT="${GUNBATTE_PORT:-8321}"

here="$(cd "$(dirname "$0")" && pwd)"

echo "▶ game host : $GUNBATTE_GAME_HOST"
echo "▶ site host : $GUNBATTE_SITE_HOST"
echo "▶ identity  : $GUNBATTE_DEPLOY_USER (ssh + service), dir $GUNBATTE_DIR, port $GUNBATTE_PORT"

# --- 0. the deploy identity ---------------------------------------------------
if ! id "$GUNBATTE_DEPLOY_USER" >/dev/null 2>&1; then
    sudo useradd -m -s /bin/bash "$GUNBATTE_DEPLOY_USER"
    echo "✓ user: $GUNBATTE_DEPLOY_USER created"
fi
sudo install -d -m 0700 -o "$GUNBATTE_DEPLOY_USER" -g "$GUNBATTE_DEPLOY_USER" \
    "/home/$GUNBATTE_DEPLOY_USER/.ssh"
pubkey_tmp="$(mktemp)"
printf '%s\n' "$GUNBATTE_DEPLOY_PUBKEY" > "$pubkey_tmp"
ak="/home/$GUNBATTE_DEPLOY_USER/.ssh/authorized_keys"
if sudo test -f "$ak" && sudo grep -qxF "$(cat "$pubkey_tmp")" "$ak"; then
    echo "✓ ssh: CI pubkey already in $GUNBATTE_DEPLOY_USER's authorized_keys"
else
    sudo tee -a "$ak" < "$pubkey_tmp" >/dev/null
    echo "✓ ssh: CI pubkey appended to $GUNBATTE_DEPLOY_USER's authorized_keys"
fi
sudo chown "$GUNBATTE_DEPLOY_USER:$GUNBATTE_DEPLOY_USER" "$ak"
sudo chmod 0600 "$ak"
sudo chown -R "$GUNBATTE_DEPLOY_USER:$GUNBATTE_DEPLOY_USER" "$GUNBATTE_DIR"
rm -f "$pubkey_tmp"

# --- 1. systemd unit (runs as the deploy identity) -----------------------------
sed -e "s|__GUNBATTE_USER__|$GUNBATTE_DEPLOY_USER|g" \
    -e "s|__GUNBATTE_DIR__|$GUNBATTE_DIR|g" \
    -e "s|__GUNBATTE_PORT__|$GUNBATTE_PORT|g" \
    "$here/gunbatte.service" | sudo tee /etc/systemd/system/gunbatte.service >/dev/null
sudo systemctl daemon-reload
sudo systemctl enable gunbatte.service
echo "✓ systemd: gunbatte.service installed + enabled"

# --- 2. sudoers drop-in: restart THIS unit only, for THIS identity only -------
sudoers_tmp="$(mktemp)"
printf '%s ALL=(root) NOPASSWD: /usr/bin/systemctl restart gunbatte.service, /usr/bin/systemctl status gunbatte.service\n' \
    "$GUNBATTE_DEPLOY_USER" > "$sudoers_tmp"
if sudo visudo -c -q -f "$sudoers_tmp"; then
    sudo install -m 0440 "$sudoers_tmp" /etc/sudoers.d/gunbatte-deploy
    echo "✓ sudoers: $GUNBATTE_DEPLOY_USER may restart/status gunbatte.service without a password"
else
    echo "!! visudo rejected the drop-in — skipping (deploys will need the sudo password)" >&2
fi
rm -f "$sudoers_tmp"

# --- 3. nginx site file (add-only) ---------------------------------------------
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

# --- 4. TLS via certbot (needs both hostnames resolving to THIS machine) --------
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

# --- 5. start / refresh the service ---------------------------------------------
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
