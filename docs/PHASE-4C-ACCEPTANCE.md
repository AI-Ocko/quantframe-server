# Phase 4c acceptance — hardening before go-live (deployed 2026-09-16 04:07 UTC; checks run 04:07–05:55 UTC)

- **Server:** `ockohome`, stack at `~/stacks/quantframe-server`, branch `phase-4c-hardening` (`11c1351`). Web UI origin `http://quantframe.cgcorp.internal`.
- **Gaming PC:** this desktop. `qf-helper` unchanged since phase 4b apart from the queue fsync (Task 6); the running binary is the 4b build, which is fine for these checks (the fsync is not observable at runtime).
- **Local gate (at `0194ca4`, unchanged by the README-only `11c1351`):**
  - Tests pass: qf_log_parser 20 (11 unit, 9 fixture), qf-helper 23, wf-market 2, qf_core 185, qf-server 19 (4 unit, 15 integration), migration 0.
  - All 77 RPC commands used by the web exist.
  - `pnpm build` (tsc and vite) is clean.
- **Deploy:**
  - `backups/` on ockohome set to mode 1777 (no passwordless sudo for `chown 10001`); the container writes into it as uid 10001.
  - rsync deleted only the removed `web/src/components/Forms/LiveScraperControl/`; `docker compose config` valid.
  - Healthy within 6 s; migration `m20260920_000001_add_helper_events_alerted_at` applied (`Database ready`); `Housekeeping Started: tick 60 s, backups in /backups`; no panic; no `Trader started`.
  - First housekeeping tick (10 s after start) wrote `/backups/quantframe-2026-09-16.sqlite` (473,001,984 bytes, owner uid 10001) and pruned 0; host-side `PRAGMA integrity_check` (read-only URI) printed `ok`.
  - Pre-deploy: 0 `needs_review` rows would have alerted on the first sweep, so no burst.
- Acceptance RPC calls were made on ockohome with a session from the container's password secret; the session file was deleted afterwards. Device keys and the web password never appeared in the conversation, logs or repo.
- The `On Alert` Discord webhook was **not** configured during acceptance, so the alert checks prove the toast event and the log line, not a Discord delivery.

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Start gate, dry-run: `auto_delete` on shows `checklist.auto_delete_off = false`, state stays available | Pass (partial) | `trader_status`: `dry_run=true`, `auto_delete_off=false`. The state read `offline` throughout because Warframe was not running (helper heartbeat says not running), so the informational-in-dry-run half is proven by the unit tests (`auto_delete_only_blocks_a_live_start`, `a_live_start_is_refused_while_auto_delete_is_on_but_a_dry_run_start_is_not`) and the field plumbing live |
| 2 | Start gate, live: dry-run off → Start refused; `auto_delete` off → Ready; restore | Pass | `trader_set_options {dryRun:false}` → `auto_delete_off=false`; `trader_start` → `Trader:Start "The trader is not ready; see the start checklist"`; `app_update_settings` with `auto_delete=false` → `auto_delete_off=true`; restored `auto_delete=true` (confirmed on `/data/settings.json`) and `dry_run=true`. Nothing was started |
| 3 | Planted `apply_failed:` row alerts once | Pass | Row `7bcd7311…` inserted with the container stopped; first tick after start: two `OnNotify` events and `Housekeeping alerts 2` (this row plus check 4's first attempt); `alerted_at 2026-09-16T05:46:09Z`; the next sweep logged nothing; row ignored afterwards |
| 4 | Planted stale `applying` row is relabelled `apply_interrupted` and alerts once | Pass | First attempt used a January date and retention deleted it in the same tick (correct behaviour, useless evidence). Re-planted `579ca3ff…` ten minutes old: first tick `Housekeeping alerts 1`, reason `apply_interrupted`, `alerted_at 2026-09-16T05:49:33Z`, one `OnNotify`; next sweep quiet; ignored → `ignored/reviewed` |
| 5 | Housekeeping runs with the collector off | Pass | Image run with no volume and `QF_COLLECTOR=off`, a scratch data dir and a 20-character scratch password: `QF_COLLECTOR=off; market data collection is disabled` then `Housekeeping Started` and a first backup in the scratch dir; stopped by `timeout 75` |
| 6 | Restore rehearsal | Pass | Helper stopped. Backup counts (read-only): helper_events 7, transaction 280, stock_item 16. Live before: 9 / 280 / 16 (the two extra events were the planted rows created after the backup). `docker compose stop`; README busybox command (copy, remove `-wal`/`-shm`, `chown 10001`); healthy in 6 s, `Database ready`, `Housekeeping Started`; live after: 7 / 280 / 16 = backup. Helper restarted, heartbeat accepted |
| 7 | Trader tolerates rows removed mid-cycle | Covered by tests | No real trade happened during acceptance; watch the first real sale with the trader running: the log must not show `ItemEntry:GetStockItem` |
| 8 | Web: Notifications tab lists On Alert; Trades tab shows `apply_interrupted`; `alerted_at` not displayed; settings save | Pass (RPC side), browser pending | `on_alert` present in the settings payload; the Trades tab reason for `579ca3ff…` was `apply_interrupted` via the RPC; `alerted_at` is type-only. **User to confirm in the browser:** Settings → Notifications shows On Alert, saving works, the Trades tab renders |

Event table after the checks (post-restore): `applied` 5, `ignored` 2. The three acceptance rows planted after the backup were removed by the restore, as expected.

## Open item found during acceptance

**Transactions 5–8 are missing.** The four real trades from the phase 4b acceptance (ids 5–8: `ammo_case` ×3 and `aero_periphery`, 2026-09-16 01:13–01:29 UTC) and their two stock rows (ids 4, 5) are absent from the live database and from today's backup; the imported desktop rows are ids 9–288 (280 rows). The pre-import backup `backups/quantframe.sqlite.pre-import-20260916T014018Z` still contains all four. The import agent verified them unchanged at about 01:45 UTC; the backup at 04:07 UTC no longer had them, and the live count read 280 before the restore rehearsal, so the restore did not cause it. No phase 4c code deletes transactions or stock rows. If the user did not delete them by hand, this is unexplained data loss that predates 4c's deploy; the rows can be re-inserted from the pre-import backup.

## Follow-ups

1. **Resolve the missing transactions 5–8** (above): confirm whether they were deleted on purpose; if not, restore them from the pre-import backup and find the cause.
2. **Import the user's existing desktop trading data** — done 2026-09-16 (280 transactions, 16 stock items; script `scripts/import-desktop-data.py`). Desktop rows 283/284 look like the same Galvanized Shot purchase logged twice (50p and 300p); the user may want to delete one.
3. **`auto_delete` is still on** in the server settings; a live start is now refused until it is turned off (H1). Decide before go-live whether the first live start should be a clean slate.
4. **Idle cycle cost** (phase 3 follow-up 4) still stands.
5. **Buy, sell and wish-list decisions haven't run on live data yet** (phase 3 check 5). Re-check once items turn warm (from about 2026-09-22).
6. **Phase 2 time-based checks** 3–5, 7 and 8 are still open.
7. **Known limitation.** Presence and trades assume one gaming PC.
8. **Partly closed.** `apply_failed` and stranded applies now alert; a review re-apply after a partial apply can still double-apply the items already written, and the modal says so.
9. **Test-fidelity note** from 4b stands: `wrapped_mod_name.log` and `split_end_marker.log` contain no actual split.
10. **Unexercised by a real trade:** a purchase matching a wish-list row; closing or lowering a real WFM order on a trade.
11. **Configure the `On Alert` Discord webhook** (Settings → Notifications → On Alert) — per-installation; without it alerts are toast and log only.
12. **Backups live on ockohome only** (no off-site copy); the folder is world-writable (1777) because the deploy user has no passwordless sudo for `chown 10001`.
13. **Deferred minors from the reviews:** `needing_alert`'s skip warning does not name the event id; `list`/`get` still fail on a corrupt event row (the Trades tab page containing it); a prune error after a good backup is reported as a backup failure; only `'` is guarded in the backup path; a `.tmp` may linger if removing a bad copy fails; `Gates` reset on a supervised restart; backup failure logs at error level (no `critical()` helper exists); `alert_variables`' `<KIND>` else-branch; `apply_reviewed` does not re-stamp `applying` before re-applying, so a crash mid-re-apply of an ordinary-reason row is never swept.
14. **Repo hygiene:** add `rustfmt.toml` with `max_width = 150` (default `cargo fmt` rewrites 62 files).
15. **From the same-day layout hotfix (branch `hotfix-live-scraper-layout`, deployed):** the Live Scraper table header no longer sticks at page size 50/100 (mantine-datatable 8 has no sticky-header option); the same `calc(100vh - var(--offset))` pattern remains on the Trade Messages, Trading Analytics and Debug pages.
