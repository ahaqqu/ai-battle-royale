#!/usr/bin/env bash
# provision/vps/doctor.sh — read-only health sweep for the ahaqqu.com VPS.
#
# VENDORED COPY — canonical source: ahaqqu/homepage provision/vps/doctor.sh
# (private repo). This snapshot ships to the box via deploy.sh's post-deploy
# gate, because the box cannot fetch from the private repo unauthenticated
# and the gate must not need a GitHub credential. Keep in sync when the
# canonical copy changes; this copy must not diverge behaviorally.
#
# The box hosts several apps (see MACHINE.md for the manifest); this script
# checks the SHARED machinery — nginx config, sites-enabled hygiene, failed
# units, certificates, disk, expected listeners — without changing anything.
# Safe to run any time, as any user; checks that need root degrade to a
# warning instead of prompting (sudo -n only, never interactive).
#
#   bash provision/vps/doctor.sh
# Exit code: 0 = healthy (warnings allowed), 1 = something needs a human.

set -uo pipefail

# Paths are overridable so the sweep can be exercised against a fake tree in
# tests; on the box the defaults are what you want.
enabled="${DOCTOR_SITES_ENABLED:-/etc/nginx/sites-enabled}"
nginx_conf="${DOCTOR_NGINX_CONF:-/etc/nginx/nginx.conf}"
replays="${DOCTOR_GUNBATTE_REPLAYS:-/home/kajianq-deploy/gunbatte/replays}"

ok=0; warn=0; fail=0
log()  { printf '\033[32m✓\033[0m %s\n' "$*"; ok=$((ok+1)); }
note() { printf '\033[33m!\033[0m %s\n' "$*"; warn=$((warn+1)); }
bad()  { printf '\033[31m✗\033[0m %s\n' "$*"; fail=$((fail+1)); }

echo "── $(hostname) · $(date '+%Y-%m-%d %H:%M %Z') · up $(uptime -p 2>/dev/null | sed 's/up //') ──"

# --- nginx config test -----------------------------------------------------------
if nginx -t >/dev/null 2>&1; then
    log "nginx: config test passes"
elif sudo -n nginx -t >/dev/null 2>&1; then
    log "nginx: config test passes (via sudo -n)"
else
    # Unverifiable (no root: ssl keys are 0600) is a warning; syntactically
    # broken is a failure. The distinction decides whether the post-deploy
    # gates in the apps' deploy scripts block or degrade.
    nt_err="$(nginx -t 2>&1 || true)"
    if printf '%s' "$nt_err" | grep -qi "permission denied"; then
        note "nginx: config test unverifiable without root — grant this identity '/usr/bin/nginx -t' (read-only) for the enforcing gate"
    else
        bad "nginx: config test FAILS — $(printf '%s' "$nt_err" | tail -n1)"
    fi
fi

# --- sites-enabled hygiene -------------------------------------------------------
# nginx includes every entry in sites-enabled/ as config; anything that is
# not an app site file (backups, editor temps) duplicates listeners box-wide.

if [ -d "$enabled" ]; then
    strays="$(find "$enabled" -mindepth 1 ! -name '*.conf' -printf '%f\n' 2>/dev/null)"
    if [ -n "$strays" ]; then
        bad "sites-enabled: non-.conf entries are parsed as config — move them out: $(echo "$strays" | tr '\n' ' ')"
    else
        log "sites-enabled: only .conf site files (nothing stray)"
    fi
    glob="$(grep -h 'include.*sites-enabled' "$nginx_conf" 2>/dev/null | tail -n1 | awk '{print $2}' | tr -d ';')"
    case "$glob" in
        *'.conf'*) log "nginx include: '$glob' — stray files cannot enter the config" ;;
        *)         note "nginx include: '${glob:-?}' is the bare glob — one stray file in sites-enabled/ breaks the whole box; narrowing it to 'sites-enabled/*.conf' is the MACHINE.md hardening" ;;
    esac
else
    bad "sites-enabled: $enabled does not exist"
fi

# --- systemd: failed units + the manifest's apps ---------------------------------
failed="$(systemctl --failed --no-legend --plain 2>/dev/null | grep -v '^$' || true)"
if [ -n "$failed" ]; then
    bad "systemd: failed units — $(echo "$failed" | tr '\n' '; ')"
else
    log "systemd: no failed units"
fi
for unit in gunbatte.service kajianq-api.service kajianq-cron.timer kajianq-backup.timer; do
    if systemctl cat "$unit" >/dev/null 2>&1; then
        state="$(systemctl is-active "$unit" 2>/dev/null || true)"
        if [ "$state" = "active" ] || [ "$state" = "waiting" ]; then
            log "$unit: active"
        else
            bad "$unit: state '$state'"
        fi
    elif [ "$unit" = "gunbatte.service" ] || [ "$unit" = "kajianq-api.service" ]; then
        bad "$unit: not installed (MACHINE.md says it should be)"
    else
        note "$unit: not installed (kajianq's timers — re-run its apply.sh if the box should have them)"
    fi
done

# --- certificates ----------------------------------------------------------------
certs="$(sudo -n certbot certificates 2>/dev/null || certbot certificates 2>/dev/null || true)"
if [ -n "$certs" ]; then
    # Expiry lines read: "Expiry Date: 2026-01-01 11:20:33+00:00 (VALID: 12 days)"
    expiring="$(echo "$certs" | awk '/Certificate Name/{n=$3} /Expiry Date/{ if ($6 + 0 < 14) print n": "($6+0)" days left" }')"
    names="$(echo "$certs" | awk '/Certificate Name/{printf "%s ", $3}')"
    if [ -n "$expiring" ]; then
        note "certbot: renewing soon — $expiring (the renew timer should handle it)"
    else
        log "certbot: lineages present, none inside 14 days ($names)"
    fi
else
    note "certbot: could not list certificates without root — run 'sudo bash provision/vps/doctor.sh' for the cert check"
fi

# --- disk + memory (disk is the scarce resource on this box) ---------------------
root_use="$(df --output=pcent / | tail -n1 | tr -dc '0-9')"
log "disk /: ${root_use}% used"
[ "$root_use" -ge 85 ] && bad "disk: / above 85% — gunbatte replays are the usual suspect"
if [ -d "$replays" ]; then
    replays_g="$(du -sB1G "$replays" 2>/dev/null | cut -f1 || echo 0)"
    if [ "${replays_g:-0}" -ge 5 ]; then
        note "gunbatte replays/: ${replays_g} GB (the retention sweep only runs at service restart — a restart will trim to the newest 100 matches)"
    else
        log "gunbatte replays/: ${replays_g} GB (under the 5 GB heads-up)"
    fi
fi
mem_avail="$(free -m | awk '/^Mem:/{print $7}')"
log "memory: ${mem_avail} MB available"

# --- expected loopback listeners (from MACHINE.md) --------------------------------
listeners="$(ss -tln 2>/dev/null | awk '{print $4}')"
check_port() { # $1 port, $2 what
    if echo "$listeners" | grep -q ":$1\$"; then
        log "listening: 127.0.0.1:$1 ($2)"
    else
        bad "listening: 127.0.0.1:$1 ($2) is NOT listening"
    fi
}
check_port 8321 "gunbatte game server"
check_port 8787 "kajianq API"
check_port 5432 "postgres (kajianq)"
# Response-level check behind the port: kajianq's own health endpoint
# (MACHINE.md). Only meaningful when the port answered above.
if echo "$listeners" | grep -q ":8787\$"; then
    if curl -fsS -m 5 http://127.0.0.1:8787/v1/health >/dev/null 2>&1; then
        log "kajianq API: /v1/health answers ok"
    else
        bad "kajianq API: 8787 is listening but /v1/health fails"
    fi
fi

echo
echo "── $ok ok · $warn warnings · $fail failures ──"
[ "$fail" -eq 0 ]
