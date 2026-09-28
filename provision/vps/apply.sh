#!/usr/bin/env bash
# provision/vps/apply.sh — one-time (and re-runnable) VPS bootstrap.
#
# Run ON the VPS, as root — provision/vps/deploy.sh --bootstrap does exactly
# that (sudo env … bash apply.sh). Running it by hand:
#   sudo -v && GUNBATTE_GAME_HOST=… GUNBATTE_SITE_HOST=… GUNBATTE_EMAIL=… \
#     GUNBATTE_DEPLOY_USER=kajianq-deploy GUNBATTE_DIR=/home/kajianq-deploy/gunbatte \
#     GUNBATTE_PORT=8321 bash apply.sh
# The CI public key is read from deploy.pub next to this script (uploaded by
# deploy.sh).
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

here="$(cd "$(dirname "$0")" && pwd)"

# The CI public key: passed via env, or read from deploy.pub beside this
# script (deploy.sh --bootstrap uploads it with the rest of the tree).
if [ -z "${GUNBATTE_DEPLOY_PUBKEY:-}" ] && [ -f "$here/deploy.pub" ]; then
    GUNBATTE_DEPLOY_PUBKEY="$(cat "$here/deploy.pub")"
fi
: "${GUNBATTE_GAME_HOST:?set GUNBATTE_GAME_HOST (game hostname, e.g. play.example.com)}"
: "${GUNBATTE_SITE_HOST:?set GUNBATTE_SITE_HOST (website hostname, e.g. gunbatte.example.com)}"
: "${GUNBATTE_DEPLOY_PUBKEY:?set GUNBATTE_DEPLOY_PUBKEY (contents of the CI public key — deploy.sh takes it from deploy.pub beside this script)}"
: "${GUNBATTE_DEPLOY_USER:?set GUNBATTE_DEPLOY_USER (restricted deploy identity, e.g. kajianq-deploy)}"
: "${GUNBATTE_DIR:?set GUNBATTE_DIR (app dir, e.g. /home/kajianq-deploy/gunbatte)}"
: "${GUNBATTE_WEB_ROOT:?set GUNBATTE_WEB_ROOT (world-readable web root, e.g. /srv/gunbatte/website)}"
: "${GUNBATTE_PORT:?set GUNBATTE_PORT (loopback port for abr-server, e.g. 8321)}"

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

# --- 2. sudoers drop-in: scoped grants for THIS identity only -----------------
# Two commands only: restart/status this unit, and publish the website from
# the deploy tree to the world-readable web root nginx serves.
sudoers_tmp="$(mktemp)"
cat > "$sudoers_tmp" <<EOF
$GUNBATTE_DEPLOY_USER ALL=(root) NOPASSWD: /usr/bin/systemctl restart gunbatte.service, /usr/bin/systemctl status gunbatte.service
$GUNBATTE_DEPLOY_USER ALL=(root) NOPASSWD: /usr/bin/rsync -a --delete $GUNBATTE_DIR/website/ $GUNBATTE_WEB_ROOT/
EOF
if sudo visudo -c -q -f "$sudoers_tmp"; then
    sudo install -m 0440 "$sudoers_tmp" /etc/sudoers.d/gunbatte-deploy
    echo "✓ sudoers: $GUNBATTE_DEPLOY_USER may restart gunbatte.service + publish the site (no password)"
else
    echo "!! visudo rejected the drop-in — skipping (deploys will need the sudo password)" >&2
fi
rm -f "$sudoers_tmp"

# --- 3. website web root (world-readable, outside the 0700 home) ---------------
sudo install -d -m 0755 "$GUNBATTE_WEB_ROOT"
sudo rsync -a --delete "$GUNBATTE_DIR/website/" "$GUNBATTE_WEB_ROOT/"
sudo find "$GUNBATTE_WEB_ROOT" -type d -exec chmod 0755 {} +
sudo find "$GUNBATTE_WEB_ROOT" -type f -exec chmod 0644 {} +
echo "✓ web root: website published to $GUNBATTE_WEB_ROOT (world-readable for nginx)"

# --- 4. nginx site file ---------------------------------------------------------
# Fresh install renders the template. An existing file is left alone EXCEPT
# for the `root` directive, which is reconciled so a web-root change reaches
# the live config without discarding certbot's TLS edits. Hostnames and TLS
# blocks are never rewritten — changing those requires `certbot --nginx` again.
if sudo test -f /etc/nginx/sites-enabled/gunbatte.conf; then
    current_root="$(sudo grep -m1 -E '^\s*root\s' /etc/nginx/sites-enabled/gunbatte.conf | awk '{print $2}' | tr -d ';')"
    if [ "$current_root" != "$GUNBATTE_WEB_ROOT" ]; then
        echo "▶ nginx: reconciling root directive ($current_root → $GUNBATTE_WEB_ROOT)…"
        # Replace every occurrence of the old path (one per server block).
        sudo sed -i "s|$current_root|$GUNBATTE_WEB_ROOT|g" \
            /etc/nginx/sites-enabled/gunbatte.conf
        sudo nginx -t
        sudo systemctl reload nginx
        echo "✓ nginx: root updated (TLS blocks untouched)"
    else
        echo "✓ nginx: gunbatte.conf already installed — leaving certbot's TLS edits alone"
    fi
else
    sed -e "s|__GUNBATTE_GAME_HOST__|$GUNBATTE_GAME_HOST|g" \
        -e "s|__GUNBATTE_SITE_HOST__|$GUNBATTE_SITE_HOST|g" \
        -e "s|__GUNBATTE_WEB_ROOT__|$GUNBATTE_WEB_ROOT|g" \
        -e "s|__GUNBATTE_PORT__|$GUNBATTE_PORT|g" \
        "$here/nginx/gunbatte.conf" | sudo tee /etc/nginx/sites-available/gunbatte.conf >/dev/null
    sudo ln -sf /etc/nginx/sites-available/gunbatte.conf /etc/nginx/sites-enabled/gunbatte.conf
    sudo nginx -t
    sudo systemctl reload nginx
    echo "✓ nginx: gunbatte site installed (HTTP only; certbot adds TLS next)"
fi

# --- 5. TLS via certbot (needs both hostnames resolving to THIS machine) --------
# Resolve via the local resolver and compare against this host's own
# addresses — read with `hostname -I`, not `ip` (which can fail under sudo's
# restricted PATH and produced a false "not this machine" skip).
resolved="$(getent hosts "$GUNBATTE_GAME_HOST" | awk '{print $1; exit}')"
own_ips="$(hostname -I 2>/dev/null || true)"
skip_certbot=0
if [ -z "$resolved" ]; then
    echo "!! DNS: $GUNBATTE_GAME_HOST does not resolve yet — skipping certbot (re-run apply.sh after DNS goes live)"
    skip_certbot=1
elif ! printf '%s\n' $own_ips | grep -qxF "$resolved"; then
    echo "!! DNS: $GUNBATTE_GAME_HOST resolves to $resolved, which is not this machine ($own_ips) — skipping certbot"
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
