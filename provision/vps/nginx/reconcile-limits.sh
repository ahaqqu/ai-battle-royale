#!/usr/bin/env bash
# provision/vps/nginx/reconcile-limits.sh — add the #37 abuse-limit blocks and
# the #38 Content-Security-Policy to an ALREADY-INSTALLED gunbatte nginx site
# file, without disturbing certbot's TLS edits.
#
# The template (gunbatte.conf in this directory) is rendered only on first
# install; certbot then edits the live file in place, so the template can
# never be re-applied wholesale. This script reconciles the add-only things a
# template may legitimately gain:
#   - the per-IP limit_conn / limit_req zones (http level)
#   - location = /ws/bot and location = /ws/spectate (inside the proxy block)
#   - client_max_body_size lowered to 1m
#   - the Content-Security-Policy header (issue #38), into every game server
#     block — after certbot that is the 443 block; the 80 redirect needs none
# Hostnames, TLS blocks, and redirects are never rewritten. Idempotent: a
# second run is a no-op. deploy.sh ships this file on every deploy; it only
# RUNS when apply.sh is re-run (nginx is admin-owned by design — the deploy
# identity's sudo grant is scoped to the app service).
#
# Usage (on the VPS):
#   sudo bash reconcile-limits.sh [--dry-run] [SITE_FILE]
# Preview the exact diff without writing or reloading:
#   sudo bash reconcile-limits.sh --dry-run
# (--dry-run is a flag on purpose: sudo strips environment variables, so a
# DRY_RUN=1 prefix would silently NOT survive `sudo bash ...`.)
set -euo pipefail

target=""
dry="${DRY_RUN:-0}"
for arg in "$@"; do
    case "$arg" in
        --dry-run) dry=1 ;;
        *) target="$arg" ;;
    esac
done
target="${target:-/etc/nginx/sites-enabled/gunbatte.conf}"
# Resolve symlinks BEFORE anything else: sites-enabled entries are normally
# symlinks into sites-available, and nginx includes every file in
# sites-enabled/ — a backup dropped next to the link would be parsed as a
# second copy of the config and fail `nginx -t` with duplicate listeners.
# (Certbot can also rewrite the entry into a REGULAR file, in which case
# nothing resolves — the backup logic below handles that explicitly.)
target="$(readlink -f "$target")"
[ -f "$target" ] || { echo "!! $target not found" >&2; exit 1; }

work="$(mktemp)"
trap 'rm -f "$work" "$work.tmp" "$work.locs" "$work.csp"' EXIT
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

# --- 4. Content-Security-Policy header (issue #38) -------------------------------
# The viewer's own defense in depth: inserted into every server block that
# serves the game (identified by its /ws/bot location — after certbot that is
# the 443 block; the 80 redirect block has no locations and needs nothing).
# The anchor is the ws/bot location line itself: nginx does not care where in
# a server block add_header sits, and every game block is guaranteed to have
# that line. The per-block marker check keeps a partially-reconciled file
# safe (a previous run leaves the header before the same anchor). Both
# markers match the DIRECTIVE, not any mention of the name — an operator
# note or a commented-out header must not read as done.
if ! grep -Eq '^[[:space:]]*add_header[[:space:]]+Content-Security-Policy' "$work"; then
    cat > "$work.csp" <<'EOF'
    # Issue #38: the browser refuses to run any script that did not ship
    # with the viewer — a name/replay escaping slip cannot execute. The
    # viewer needs only its own files, Google Fonts, and same-origin
    # sockets ('self' covers wss:// to this host; a scheme source like bare
    # wss: would allow sockets to ANY host); img-src data: is the inline
    # SVG favicon.
    # KEEP IN SYNC with the template copy in gunbatte.conf: certbot owns the
    # live file, so this reconcile is the only way the header reaches it.
    add_header Content-Security-Policy "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' fonts.googleapis.com 'unsafe-inline'; font-src fonts.gstatic.com; connect-src 'self'; img-src 'self' data:" always;

EOF
    awk -v cspfile="$work.csp" '
        {
            if ($0 ~ /^[[:space:]]*server[[:space:]]*\{/) inhdr = 0
            if ($0 ~ /^[[:space:]]*add_header[[:space:]]+Content-Security-Policy/) inhdr = 1
            if (!inhdr && index($0, "location = /ws/bot") > 0) {
                while ((getline l < cspfile) > 0) print l
                close(cspfile)
                inhdr = 1
            }
            print
        }
    ' "$work" > "$work.tmp"
    mv "$work.tmp" "$work"
    echo "▶ CSP: Content-Security-Policy inserted into the game server block(s)"
fi

# --- 5. preview, no-op (with a config sanity check), or commit -------------------
if diff -q "$target" "$work" >/dev/null 2>&1; then
    echo "✓ nginx: nothing to reconcile"
    # The file may be fine while the DIRECTORY around it is not (a stray file
    # in sites-enabled/ broke this exact path once) — surface it.
    nginx -t
    exit 0
fi
if [ "$dry" = 1 ]; then
    echo "── dry run: nothing written, no reload. Planned changes: ──"
    diff -u "$target" "$work" || true
    exit 0
fi
# The backup must NEVER land under sites-enabled/ — nginx include-globs that
# directory wholesale, so even a correctly-restored situation leaves a second
# copy of the config behind and `nginx -t` fails box-wide (seen live
# 2026-10-01, and again 2026-10-02 when certbot had turned the site-enabled
# entry into a REGULAR file: readlink -f then resolves to sites-enabled
# itself, and "beside the target" would mean inside the glob). Backups go
# beside the target only when that is outside sites-enabled; otherwise /root.
case "$target" in
    /etc/nginx/sites-enabled/*) backup_dir="/root" ;;
    *) backup_dir="$(dirname "$target")" ;;
esac
backup="$backup_dir/$(basename "$target").bak-$(date +%Y%m%d-%H%M%S)"
cp "$target" "$backup"
echo "▶ backup: $backup"
tee "$target" < "$work" >/dev/null
echo "✓ nginx: limits reconciled (backup: ${backup})"
if ! nginx -t; then
    echo "!! nginx -t rejected the reconciled file — restoring the backup" >&2
    cp "$backup" "$target"
    if nginx -t; then
        echo "✓ previous config restored (still live — nginx was never reloaded)"
    else
        echo "!! restored file ALSO fails nginx -t — restore by hand from ${backup}" >&2
    fi
    exit 1
fi
systemctl reload nginx
echo "✓ nginx reloaded"
