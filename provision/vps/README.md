# provision/vps — deploy GUNBATTE ROYALE to the VPS

Target topology (cohabiting with the existing apps — add-only, nothing shared
is ever edited):

```
internet ──▶ nginx :80/:443 (shared; kajianq.conf + gunbatte.conf by Host header)
                ├── play.<domain>  ──proxy──▶ 127.0.0.1:8321  abr-server (gunbatte.service)
                └── site.<domain>  ──static── $GUNBATTE_DIR/website/
```

State lives in `$GUNBATTE_DIR` (default `~/gunbatte`): the binary, `viewer/dist/`,
`website/`, and the server's own data (`ladder.db`, `replays/`).

## One-time bootstrap

DNS first: point `play.<domain>` and `site.<domain>` (A records) at the VPS.
Then, from the repo root:

```sh
GUNBATTE_GAME_HOST=play.example.com \
GUNBATTE_SITE_HOST=gunbatte.example.com \
GUNBATTE_EMAIL=you@example.com \
./provision/vps/deploy.sh --bootstrap
```

This builds the viewer + server locally (no Rust toolchain needed on the VPS),
uploads everything, then runs `apply.sh` on the VPS over ssh — it will prompt
for the sudo password once. `apply.sh`:

1. installs `gunbatte.service` (systemd, dedicated user, binds `127.0.0.1`)
2. installs a sudoers drop-in: passwordless `systemctl restart/status
   gunbatte.service` for the deploy user **only**
3. installs the nginx site file (add-only; `nginx -t` before reload)
4. issues Let's Encrypt certificates via `certbot --nginx` (skipped with a
   warning until both hostnames resolve to the VPS — just re-run)
5. starts the service

## Updates

```sh
GUNBATTE_GAME_HOST=play.gunbatte.ahaqqu.com ./provision/vps/deploy.sh
```

No sudo, no prompts: the sudoers drop-in covers the restart. The server's
state (`ladder.db`, `replays/`) is never touched by uploads.

## Automatic deploys (GitHub Actions)

`.github/workflows/deploy.yml` fires **after CI finishes green on main** and
runs the same `deploy.sh` — one deploy path for CI and humans. **All values
live as repository variables** (Settings → Secrets and variables → Actions):
`VPS_USER`, `VPS_HOST`, `GUNBATTE_GAME_HOST`, `GUNBATTE_SITE_HOST`,
`VPS_ADMIN_USER`, `GUNBATTE_PORT`. deploy.sh reads the same variables via
`gh`, so there is exactly one source of configuration. One-time key setup:

```sh
# 1. a dedicated, revocable keypair for the deploy identity (no passphrase):
ssh-keygen -t ed25519 -f ~/.ssh/gunbatte-deploy -N "" -C "gunbatte-vps-deploy-key"

# 2. pin the host key:
ssh-keyscan 62.83.35.220 > /tmp/vps_known_hosts
```

Then add two GitHub **repository secrets** (Settings → Secrets and variables →
Actions):

| Secret | Value |
|---|---|
| `VPS_DEPLOY_SSH_KEY` | contents of `~/.ssh/gunbatte-deploy` (the private key) |
| `VPS_KNOWN_HOSTS` | contents of `/tmp/vps_known_hosts` |

The **public** half is installed into the deploy identity's
`authorized_keys` by `--bootstrap` itself (idempotent append — kajianq's own
key line is kept). After that, every merge to main that passes CI deploys
itself. To revoke CI's access, delete its line from the deploy identity's
`authorized_keys` on the VPS. Until the first `--bootstrap` has run, the
workflow only uploads artifacts and prints a reminder — the final restart
needs the service to exist.

## Files

| File | Runs on | Purpose |
|---|---|---|
| `deploy.sh` | local | build + upload + (bootstrap or restart) |
| `apply.sh` | VPS | install unit / sudoers / nginx / certs, start service |
| `gunbatte.service` | — | systemd unit template (`__GUNBATTE_*__` placeholders) |
| `nginx/gunbatte.conf` | — | nginx site template (HTTP-only; certbot adds TLS in place) |

## Why this can't disturb the other apps

- One new nginx file, symlinked into `sites-enabled/`; existing server blocks
  are never edited, and `nginx -t` gates every reload (reload is graceful).
- Own systemd unit on its own loopback port; restarts affect only this app.
- Separate Let's Encrypt lineage; certbot edits only the gunbatte site file.
- The `--bind 127.0.0.1` flag keeps the raw game port off the public
  interface, so nginx+TLS is the only way in.
