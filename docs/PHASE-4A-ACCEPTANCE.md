# Phase 4a acceptance — helper link accepted (deployed 2026-09-15 07:49 UTC)

- **Server:** `ockohome`, stack at `~/stacks/quantframe-server`, branch `phase-4a-helper-link` (`22a3671`)
- **Gaming PC:** this desktop (`omarchy`, Arch Linux, Warframe under Proton). `qf-helper` 0.1.0 is a release build in `~/.local/bin`, run by the `systemd --user` unit `qf-helper.service`.
- **Local gate:**
  - Tests pass: wf-market 2, qf_core 129, qf-server 14 (3 unit, 11 integration), qf-helper 11.
  - All 74 RPC commands used by the web exist.
  - `pnpm build` (tsc and vite) is clean.
- **Deploy:**
  - rsync deleted only stale test logs under `crates/qf_core/logs/2026-09-14`.
  - The container was healthy within 37 s, and the migrations applied (`Database ready`).
  - There was no panic and no trader start at boot.
  - `POST /helper/heartbeat` answered 401 with no key and with a made-up key.
- Device keys never appeared in the conversation, logs or repo. The config is `~/.config/qf-helper/qf-helper.toml`, mode 600.

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Before any heartbeat: both helper items ✕, Offline, no override switch, "no heartbeat" line | Pass | Confirmed by the user |
| 2 | Creating `gaming-pc` shows the key once with a config snippet; the device is listed Active | Pass | Confirmed by the user. The config was saved by hand (mode 600) |
| 3 | `qf-helper --once` and the service: "qf-helper connected" ✓, "Warframe running" ✕, last heartbeat filled in | Pass | `--once` printed `Warframe running: no; heartbeat accepted` with exit 0. The service went active at 07:57:35 UTC. Confirmed by the user in the browser |
| 4 | Launching Warframe: "Warframe running" ✓ within ~10 s, state Ready | Pass | The helper logged `Warframe running: yes` at 07:58:55 UTC. The matched process's argv[0] is a Windows path whose file name is `Warframe.x64.exe`. Confirmed by the user |
| 5 | Start in dry-run, then quit Warframe: stops with "Warframe closed on the gaming PC" | Pass | `Trader started (dry-run)` at 07:59:30; the helper reported the game closed at 07:59:45; `Trader stopped: Warframe closed on the gaming PC` at 07:59:48 |
| 6 | Relaunch, Start, stop the helper: stops with "No qf-helper heartbeat for more than 60 s" | Pass | Started at 08:01:12; `systemctl --user stop qf-helper` at 08:01:47; `Trader stopped: No qf-helper heartbeat for more than 60 s` at 08:02:48. The service was started again afterwards |
| 7 | Revoke the key: the helper logs a 401 and the checklist goes ✕; a new key is accepted | Pass | After the revoke the helper logged `device key rejected (401), retrying every 60 s` at 08:03:51 UTC. The user saw ✕. With `gaming-pc-2` in the config, a restart logged `heartbeat accepted` at 08:05:55 |
| 8 | `docker compose restart`: Offline until the next heartbeat, then Ready; never Trading | Pass | Restarted at 08:06:08; `Core started` at 08:06:22; no `Trader started` afterwards. The helper logged no failures across the restart. The user saw Ready after logging in again (sessions are in memory) |
| 9 | Settings still save; the Dry-run log loads; devices list `gaming-pc-2` Active and `gaming-pc` Revoked | Pass | Confirmed by the user |

## Follow-ups

1. **Phase 4b plan.** Trade events: WFCD `warframe-items` and `overrides.toml`, `qf_log_parser`, `POST /helper/trade`, `helper_events`, trade resolution, and the review modal. The current EE.log has no trade lines, so the fixtures must be reconstructed from upstream markers and one real trade verified at acceptance.
2. **Import the user's existing desktop trading data** (user request, 2026-09-14). The source is `~/.local/share/dev.kenya.quantframe/quantframeV2.sqlite`, with `transaction` 280 rows and `stock_item` 16 rows. Not scheduled into a phase yet.
3. **`auto_delete` is still on** in the server settings. Live, it deletes every non-blacklisted real order when the trader starts. Decide before phase 5.
4. **Idle cycle cost** (phase 3 follow-up 4) still stands.
5. **Buy, sell and wish-list decisions haven't run on live data yet** (phase 3 check 5, limited). Re-check once items turn warm (from about 2026-09-22).
6. **Phase 2 time-based checks** 3–5, 7 and 8 are still open.
7. **Leftover setting.** `log_settings.ee_log_path` (Settings → Advanced → Log) does nothing on the server; the helper reads `ee_log_path` from `qf-helper.toml`. Remove or relabel it in 4b.
8. **Known limitation.** Presence is "last heartbeat wins" across devices. That's fine for one gaming PC; revisit if a second helper is added.
