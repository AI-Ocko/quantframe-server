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
- **Backups:** the `qf-data` volume holds `quantframe.sqlite`, and a `quantframe.sqlite_backup` copy is made on every start.

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
