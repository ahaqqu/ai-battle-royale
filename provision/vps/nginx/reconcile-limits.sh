#!/usr/bin/env bash
# provision/vps/nginx/reconcile-limits.sh — add the #37 abuse-limit blocks to
# an ALREADY-INSTALLED gunbatte nginx site file, without disturbing certbot's
# TLS edits.
#
# The template (gunbatte.conf in this directory) is rendered only on first
# install; certbot then edits the live file in place, so the template can
# never be re-applied wholesale. This script reconciles the one add-only
# thing a template may legitimately gain:
#   - the per-IP limit_conn / limit_req zones (http level)
#   - location = /ws/bot and location = /ws/spectate (inside the proxy block)
#   - client_max_body_size lowered to 1m
# Hostnames, TLS blocks, and redirects are never rewritten. Idempotent: a
# second run is a no-op. deploy.sh ships this file on every deploy; it only
# RUNS when apply.sh is re-run (nginx is admin-owned by design — the deploy
# identity's sudo grant is scoped to the app service).
#
# Usage (on the VPS):
#   sudo bash reconcile-limits.sh [/etc/nginx/sites-enabled/gunbatte.conf]
# Preview the exact diff without writing or reloading:
#   DRY_RUN=1 sudo bash reconcile-limits.sh
set -euo pipefail

target="${1:-/etc/nginx/sites-enabled/gunbatte.conf}"
dry="${DRY_RUN:-0}"
[ -f "$target" ] || { echo "!! $target not found" >&2; exit 1; }

work="$(mktemp)"
trap 'rm -f "$work" "$work.tmp" "$work.locs"' EXIT
cp "$target" "$work"

# A partially-reconciled file is fine: each piece checks its own marker, so a
# re-run after any interruption only adds what is missing.

# --- 1. rate-limit zones at http level -----------------------------------------
if ! grep -q 'zone=gunbatte_ws_conn' "$work"; then
    if grep -q '^# --- game:' "$work"; then
        marker='^# --- game:'
    else
        # Very old template without the section comment: first server block.
        marker='^server \{'
    fi
    ZONES='# Abuse limits (issue #37): per-IP ceilings on the WebSocket endpoints,
# which the app deliberately does not do itself — behind this proxy the socket
# address the app sees is 127.0.0.1.
limit_conn_zone $binary_remote_addr zone=gunbatte_ws_conn:10m;
limit_req_zone  $binary_remote_addr zone=gunbatte_ws_open:10m rate=10r/m;

' MARKER="$marker" awk '
        !done && $0 ~ MARKER { printf "%s", ENVIRON["ZONES"]; done = 1 }
        { print }
    ' "$work" > "$work.tmp"
    mv "$work.tmp" "$work"
    echo "▶ zones: limit_conn/limit_req zones inserted"
fi

# --- 2. upload body cap ----------------------------------------------------------
if ! grep -q 'client_max_body_size 1m;' "$work"; then
    if grep -q 'client_max_body_size' "$work"; then
        # First occurrence only (the game server block; the site block has none).
        sed -i '0,/client_max_body_size [^;]*;/s//client_max_body_size 1m;/' "$work"
        echo "▶ body cap: client_max_body_size → 1m"
    else
        echo "!! body cap: no client_max_body_size directive found — skipped" >&2
    fi
fi

# --- 3. throttled WS locations inside the proxy server block ---------------------
if ! grep -q 'location = /ws/bot' "$work"; then
    proxy_line="$(grep -n -m1 'proxy_pass http://127\.0\.0\.1:[0-9]*;' "$work" | cut -d: -f1 || true)"
    [ -n "$proxy_line" ] || { echo "!! locations: no proxy_pass to 127.0.0.1 found — aborting, nothing written" >&2; exit 1; }
    port="$(sed -n "${proxy_line}p" "$work" | grep -o '127\.0\.0\.1:[0-9]*' | cut -d: -f2)"
    # The proxy `location /` is the anchor: exact-match locations must sit in
    # the SAME server block, so anchor on the one whose window contains this
    # proxy_pass (certbot's 80→443 redirect block has no location at all).
    loc_line=""
    for ln in $(grep -n '^[[:space:]]*location / {' "$work" | cut -d: -f1); do
        if sed -n "${ln},$((ln + 14))p" "$work" | grep -q "proxy_pass http://127.0.0.1:${port};"; then
            loc_line="$ln"
            break
        fi
    done
    [ -n "$loc_line" ] || { echo "!! locations: proxy location / not found — aborting, nothing written" >&2; exit 1; }

    cat > "$work.locs" <<EOF
    # WebSocket endpoints: long-lived (per-IP conn ceiling) with throttled
    # handshakes (req ceiling). Exact matches win over location / below.
    location = /ws/bot {
        limit_conn gunbatte_ws_conn 10;
        limit_req  zone=gunbatte_ws_open burst=20 nodelay;
        proxy_pass http://127.0.0.1:${port};
        proxy_http_version 1.1;
        proxy_set_header Host      \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header Upgrade    \$http_upgrade;
        proxy_set_header Connection \$gunbatte_connection_upgrade;
        proxy_read_timeout  3600s;
        proxy_send_timeout  3600s;
        proxy_buffering off;
    }

    location = /ws/spectate {
        limit_conn gunbatte_ws_conn 10;
        limit_req  zone=gunbatte_ws_open burst=20 nodelay;
        proxy_pass http://127.0.0.1:${port};
        proxy_http_version 1.1;
        proxy_set_header Host      \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header Upgrade    \$http_upgrade;
        proxy_set_header Connection \$gunbatte_connection_upgrade;
        proxy_read_timeout  3600s;
        proxy_send_timeout  3600s;
        proxy_buffering off;
    }

EOF
    head -n $((loc_line - 1)) "$work" > "$work.tmp"
    cat "$work.locs" >> "$work.tmp"
    tail -n +"$loc_line" "$work" >> "$work.tmp"
    mv "$work.tmp" "$work"
    echo "▶ locations: /ws/bot + /ws/spectate inserted into the proxy block (port ${port})"
fi

# --- 4. preview or commit --------------------------------------------------------
if diff -q "$target" "$work" >/dev/null 2>&1; then
    echo "✓ nginx: nothing to reconcile"
    exit 0
fi
if [ "$dry" = 1 ]; then
    echo "── dry run: nothing written, no reload. Planned changes: ──"
    diff -u "$target" "$work" || true
    exit 0
fi
backup="${target}.bak-$(date +%Y%m%d-%H%M%S)"
cp "$target" "$backup"
tee "$target" < "$work" >/dev/null
echo "✓ nginx: limits reconciled (backup: ${backup})"
nginx -t
systemctl reload nginx
echo "✓ nginx reloaded"
