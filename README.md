# quantframe-server

A headless, self-hosted server version of [Quantframe](https://github.com/Kenya-DK/quantframe-react) by Kenya-DK,
for one user on a home network. Forked from quantframe-react at commit `3d59c4e7` (v1.6.28).

Licensed under GPLv3, like the upstream project.

Design: `docs/superpowers/specs/2026-09-14-quantframe-server-design.md`.

## Running on the homelab

Docker runs on the server only.

```bash
# copy the repo to the server, e.g. ~/stacks/quantframe-server, then on the server:
cd ~/stacks/quantframe-server
cp .env.example .env            # edit QF_PUBLIC_ORIGIN
mkdir -p secrets
openssl rand -hex 32 > secrets/qf_secret_key
printf '%s' 'choose-a-long-password' > secrets/qf_web_password   # at least 12 characters
sudo chown 10001:10001 secrets/* && sudo chmod 600 secrets/*
docker compose up -d --build
docker compose logs -f
```

Open `QF_PUBLIC_ORIGIN` in a browser, sign in with the web password, then sign in to warframe.market.
Only the warframe.market token is kept, encrypted with `qf_secret_key`. Your warframe.market password is never stored.

- **Changing the web password:** edit `secrets/qf_web_password` and restart the container.
- **Losing `qf_secret_key`:** the stored token can't be decrypted, so sign in to warframe.market again.
- **Backups:** every UTC day the server writes `backups/quantframe-<YYYY-MM-DD>.sqlite` (a `VACUUM INTO` copy, integrity-checked) into the host folder mounted at `/backups`, keeping the last 7 days and the last 4 Sundays. Create the folder once, owned by the container's uid: `mkdir -p backups && sudo chown 10001 backups` (or `chmod 1777 backups` without sudo). A failed backup logs a Critical line, shows a red toast and sends the `On Alert` notification; it is retried the next day. The start-time `quantframe.sqlite_backup` copy in the volume is only a migration safety net.
- **Restore:** First check the file you are about to restore: `python3 -c "import sqlite3,sys; print(sqlite3.connect('file:'+sys.argv[1]+'?mode=ro', uri=True).execute('PRAGMA integrity_check').fetchone()[0])" backups/quantframe-<date>.sqlite` must print `ok`. Then `docker compose stop`, then copy the chosen file over the live database and drop the stale WAL:
  ```bash
  docker run --rm -v quantframe-server_qf-data:/data -v "$PWD/backups:/b:ro" busybox sh -c \
    'cp /b/quantframe-<date>.sqlite /data/quantframe.sqlite && rm -f /data/quantframe.sqlite-wal /data/quantframe.sqlite-shm && chown 10001 /data/quantframe.sqlite'
  docker compose up -d
  ```
  Then confirm `Database ready` in the log and that the Trades tab and transaction counts match the backup's date. (The volume name is `<stack folder>_qf-data`; on ockohome the stack folder is `quantframe-server`.)
- **Going live:** follow `docs/GO-LIVE-RUNBOOK.md` once the dry-run review passes. Nothing turns dry-run off automatically.

## qf-helper (gaming PC)

`qf-helper` runs on the PC that plays Warframe. Every 10 s it tells the server whether Warframe is running. The trader is only Ready while those heartbeats arrive, and it stops when Warframe closes or the heartbeats stop for more than 60 s.

1. In the web UI, open **Live Scraper → Helper devices**, create a device and copy the `qf-helper.toml` it shows. The key is shown only once.
2. Build and install it natively (not in Docker):
   ```bash
   cargo build --release -p qf-helper
   install -Dm755 target/release/qf-helper ~/.local/bin/qf-helper
   install -Dm600 /dev/stdin ~/.config/qf-helper/qf-helper.toml   # paste the config, then Ctrl-D
   ~/.local/bin/qf-helper --once                                   # prints the state and exits 0 when accepted
   ```
3. Run it as a user service:
   ```bash
   install -Dm644 contrib/qf-helper.service ~/.config/systemd/user/qf-helper.service
   systemctl --user daemon-reload
   systemctl --user enable --now qf-helper
   journalctl --user -u qf-helper -f
   ```

`qf-helper.toml`:

```toml
server_url = "http://ockohome:8080"
device_key = "qfh_…"
# ee_log_path = "/path/to/EE.log"   # optional; defaults to the Proton path under ~/.local/share/Steam
```

### Trade reporting

`qf-helper` also follows `EE.log` from the moment it starts (earlier trades are never replayed) and reports every trade Warframe confirms with "The trade was successful!" to the server. The server records the transaction, updates stock and your real warframe.market order, or parks the trade under **Live Scraper → Trades** for review when a name doesn't resolve.

- Events the server hasn't accepted yet wait in `~/.local/state/qf-helper/trade-queue.jsonl` and are replayed in order.
- `qf-helper --parse /path/to/EE.log` prints the trades found in a log file as JSON, without a server.
- The journal shows one line per trade: `trade detected: sale 70p with <player>, 1 items; server: applied`.

**Names that don't resolve.** The server matches in-game names against warframe.market's English names. For the rare miss, create `overrides.toml` in the data volume (`docker compose exec quantframe-server sh -c 'cat > /data/overrides.toml'` or edit it on the host) mapping the in-game name to the warframe.market slug:

```toml
[names]
"Primed Fir" = "primed_firestorm"
```

It is read on every trade, so no restart is needed. The review modal shows the exact name the game sent.
