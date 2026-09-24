# Phase 5 acceptance — go-live prep accepted (deployed 2026-09-16 10:02 UTC; checks run after)

This record covers the **prep** half of phase 5 only (spec §21 G1 to G4). The flip has **not** happened. It happens on or after 2026-09-22 and is appended to the last section of this file.

- **Server:** `ockohome`, stack at `~/stacks/quantframe-server`, branch `phase-5-go-live` (`6006abd`, merge base with main `0a8b114`). Web UI origin `http://quantframe.cgcorp.internal`.
- **Local gate (at `6006abd`):**
  - Tests pass: `utils` 3, `qf_core` 191, `qf-server` 19 (4 unit, 15 integration). One `qf_core` run had `events::tests::emitted_frames_reach_subscribers` fail; it passes 3/3 alone and 191/191 on rerun. Pre-existing flake, phase 5 did not touch `events.rs`. Follow-up 1.
  - `79 server commands, 79 used by web, 0 missing` (78 in 4d; `trader_dry_run_summary` is the new one).
  - `pnpm build` (tsc and vite) is clean, built in 659 ms. Only pre-existing `unused import` warnings; nothing new.
- **Deploy:**
  - The agent's `rsync` was **denied twice by the Claude Code auto-mode classifier** ("Production Deploy", no prompt was shown). The user ran the `rsync` and `docker compose up -d --build` manually instead.
  - The dry run beforehand listed 3 deletions: the stale in-tree runtime logs `crates/qf_core/logs/2026-09-15/trader_item.log`, `crates/qf_core/logs/2026-09-15/log.log` and the directory itself. All three happened.
  - The agent verified afterwards: a re-run of the dry run had nothing to transfer and 0 deletions, so the host tree is byte-identical to `6006abd`. `backups/` (1777), `secrets/` (0700) and `.env` (md5 `84b5aac6560f8a761926f3deccc71f83`) are unchanged.
  - **Healthy in 5.2 s:** `StartedAt` 2026-09-16T10:02:15.278Z, first health check passed 10:02:20.449Z.
  - Boot log: `[Db:Connect] Database ready` (10:02:16.218Z), `Loaded 3840 tradable items` (10:02:16.864Z), `[Collector] Started: 3840 items, 24 hot` (10:02:18.682Z), `[Housekeeping] Started: tick 60 s, backups in /backups` (10:02:18.690Z), `[Startup] Core started` (10:02:18.746Z), `listening on 0.0.0.0:8080` (10:02:18.747Z). No panic, no `CRITICAL`, no `Trader started` at boot. The one line matching `Error` is `[Emit:SendEvent:App:Error] Event: message (0 receivers)` at 10:02:18.350Z, which is `clear_error!` at startup, the same line documented in `docs/PHASE-4D-ACCEPTANCE.md`. Not a failure.
- The trader was in dry-run for the whole acceptance window. Nothing was started live and no real order was touched.

| # | Check (spec §21 G7, "now") | Result | Notes |
|---|---|---|---|
| 1 | A dry-run Start logs simulated deletes instead of real ones | Pass | The Start logged `Simulated delete of order … (global) N/50` lines. No `Deleted order with ID` line appeared |
| 2 | With nothing to process, `Checking items...` repeats about every 30 s | Pass | The idle cadence was about 30 s throughout the check |
| 3 | Stop still takes effect within about a second | Pass | Stop was pressed about 10 s into an idle pause. The badge left Trading at once and `Trader stopped: Stop button` was logged within about 1 s |
| 4 | The Dry-run log tab shows summary tables for 1, 7 and 30 days | Pass | The Summary block with the 1/7/30 selector renders and its counts match the paged rows below it |
| 5 | The runbook exists and its pre-flight list matches the UI | Pass | Every label in `docs/GO-LIVE-RUNBOOK.md` matches what the UI shows |

All five checks were confirmed by the user in the browser. The trader stayed in dry-run for all of them; nothing was run live.

## Rulings made during execution

- The `Deleted order for item` line at `helpers.rs:297` prints after a simulated delete too. That is the same defect as G1, so it was fixed in the same task with the same DryRun/Live wording split. G1 names only the auto-delete loop, but its intent covers every delete line.
- `scripts/check-rpc-commands.py` counts a web call with no server command as "missing", not the reverse. The plan's expectation of "1 missing" after Task 3 was therefore wrong. A server-only command shows up as the "used by web" count lagging the server count instead. The plan text stands as written; no code changed.
- The reviewer's Important on the Task 3 test stands. The plan's own filler count of 30 pushed the only NULL-price group past the `LIMIT 25`. The plan's test was wrong, not the implementer; the fix asserts the delete group directly and brings the NULL-price group inside the limit.
- The runbook deviates from the plan draft wherever the draft named surfaces that do not exist. The tab is "Log", not "Server log". The ingame status set is not logged, because `user_set_status` is silent, so the profile is checked instead. Token expiry has no UI surface and the alert is called "On Sign-in Expiring". G4's intent, watching the Log tab and warframe.market, still holds. Accepted.
- `events::tests::emitted_frames_reach_subscribers` is a pre-existing flake, not a blocker. It takes the first frame off the shared broadcast channel that the 4d log sink also feeds. It passes 3/3 alone and the full suite passes on rerun, and phase 5 did not touch `events.rs`. Recorded as follow-up 1.
- The boot line `[Emit:SendEvent:App:Error] Event: message (0 receivers)` is `clear_error!` at startup, already documented in the 4d deploy notes. Pre-existing, not a failure.

## Follow-ups

1. **Flaky `events::tests::emitted_frames_reach_subscribers`.** It takes the first frame off the shared broadcast channel that the log sink also feeds, so a log line can win the race. Filter for `channel == "message"` the way its sibling test does.
2. **Deferred minors from the task reviews.** The final review triaged all of them as "stays deferred", except the Task 5 one below that this record closes. Task 1: `helpers.rs` `progress_order` Update and Delete wording has no direct test, because that module has no `TradeContext` fixture. Task 2: the stop test clears the flag at 1.5 s, so a 2 s slice would still pass, and should stop early in the first slice, for example at 100 ms; the inline `Duration::from_secs(1)` slice could be a named const `STOP_SLICE`; there is no test for an `Err` immediately after an `Ok(0)` cycle; `item.rs` `check`'s doc comment says "processed" but the function returns the count handed in. Task 3: `by_action`'s `ORDER BY` across differing `side` and `forced_by` values is unexercised by tests; there is no empty-log test for `dry_run_summary`. Task 4: "Last 1 days" needs an i18next plural key; the two grow-equal summary tables may crowd on narrow viewports. Task 5: `en.json`'s `on_token_expiring_title` key renders "On Sign-in Expiring", a stale key name. (Task 5's other minor, that the runbook links a `docs/PHASE-5-ACCEPTANCE.md` that did not exist yet, is closed by this file.)
3. **`crates/*/logs` rides along on every rsync.** Add `--exclude 'crates/*/logs'` to the deploy command so stale in-tree runtime logs stop showing up in the deletion list.
4. **The Log tab's `useTranslate` helpers are named like hooks but are plain functions** (final review note).
5. **Still open from `docs/PHASE-4D-ACCEPTANCE.md`**, carried forward by number. Follow-ups 1 and 2 there are closed: 1 by the user's re-test of the Log tab auto-scroll, 2 by phase 5's G1.
   - 3. `RequestError.content` is logged unmasked, so it appears in full in the second line of a large error in the Log tab.
   - 5. Deferred minors from the 4d reviews (nine items, none merge-blocking).
   - 6. `auto_delete` is still on in the server settings; a live start is refused until it is off. See follow-up 6 below, which is now decided.
   - 7. Idle cycle cost (phase 3 follow-up 4).
   - 8. Buy, sell and wish-list decisions have not run on live data yet; re-check once items turn warm, from about 2026-09-22.
   - 9. Phase 2 time-based checks 3 to 5, 7 and 8 are still open.
   - 10. Known limitation: presence and trades assume one gaming PC.
   - 11. Partly closed: a review re-apply after a partial apply can still double-apply the items already written.
   - 12. Test-fidelity note from 4b: `wrapped_mod_name.log` and `split_end_marker.log` contain no actual split.
   - 13. Unexercised by a real trade: a purchase matching a wish-list row, and closing or lowering a real WFM order on a trade.
   - 14. Configure the `On Alert` Discord webhook (Settings, Notifications, On Alert); per-installation.
   - 15. Backups live on ockohome only, with no off-site copy, and the folder is world-writable (1777).
   - 16. Deferred minors from the 4c reviews (nine items).
   - 17. Repo hygiene: add `rustfmt.toml` with `max_width = 150`.
   - 18. From the same-day layout hotfix: the Live Scraper table header no longer sticks at page size 50 or 100, and the `calc(100vh - var(--offset))` pattern remains on three other pages.
6. **`auto_delete` decision made.** It is turned **off** before the flip, so the first live start adopts the existing real orders instead of deleting them (spec §21). This decides 4d follow-up 6.

## The flip (to be appended on or after 2026-09-22)

Not done yet. Follow `docs/GO-LIVE-RUNBOOK.md` and record the outcome here.

## The flip (2026-09-23)

- **When:** 2026-09-23 03:53:57 UTC (2026-09-22 20:53 Pacific). The user pressed Start with global Dry-run off after about 30 h of closed-mode dry run on the final settings; the runbook's 48 h figure was the controller's conservative number and was waived by the user. The trader set the warframe.market status to `ingame` at 03:53:57.
- **Build and settings:** ockohome `8256177` (main `2bc2cf9`). Price source `closed`, profit basis `range`, `max_buy_candidates` −1 at the flip (lowered to 120 at 04:15 UTC, see below), `trading_tax_cap` −1, `max_total_price_cap` −1, `avg_price_cap` 600, Auto Delete off (existing orders adopted), Delete buy orders on stop off.
- **First cycle:** 202 items, every one `Route: Live`. First live order write at 03:53:59 (an update of the adopted Aeolak Receiver Blueprint sell order); first live order **created** at 03:55:40, id `6ab34d8090780835bc8dca1a`.
- **First hour (03:54–04:54 UTC), from the server log and the helper journal:** 23 cycles of about 2.5 min; 82 buy orders created, 11 sell listings created, 1 648 updates, 45 price-rule deletes, 32 orphan-sweep deletes, 74 fast-drop-guard firings; **25 real trades applied automatically** from `EE.log` (13 purchases for 1 229 p, 12 sales for 1 163 p), none parked for review; 0 errors, 0 `CRITICAL`, no `Trader stopped`. One failed write: a `Not found` on PATCH of the Blind Rage buy order at 04:00:04, because the helper-detected purchase had closed that order 75 s earlier; benign, one failure, reset by the next success.
- **What the hour showed that dry-run could not:**
  1. **warframe.market's per-account order cap binds.** With no candidate limit, 105 of 202 candidates were skipped per cycle with `has reached the order limit`, and bought stock could not be listed for sale (Blind Rage waited 35 min for a slot). The user set `max_buy_candidates` to 120 at 04:15 UTC; the orphan sweep removed the 29 dropped bids at about 04:47, and skips fell to 0–11 per cycle. **Runbook rule added below:** keep the candidate limit about 30 below the account's order cap so sales always have a slot.
  2. **Orphan sweep, live:** 3 stale adopted orders deleted at 04:30 (Akbolto Prime Set 73 p, Gara Prime Set 55 p, Equilibrium 30 p; the other six of the nine judged stale had already been closed by trades or price rules), then the 29 dropped bids at 04:47. 0 failures.
  3. **Defect: set folding ignores a part's quantity in the set** (`helper_link/trades/sets.rs::fold_sets`). A 51 p purchase of a Kogake Prime Set arrived as Blueprint ×1, Gauntlet ×2, Boot ×2; the fold took one of each part and recorded the second Gauntlet and Boot as separate stock, which the trader then tried to list. warframe.market's item detail carries `quantityInSet` (gauntlet: 2), which the set cache does not read. The two rows are for the user to delete by hand; the fix (fold with per-part quantities) is pending the user's go-ahead. Recording only, no trading impact.
- **Manual cleanup:** the two Kogake part rows, to be deleted by the user. No orders were removed by hand.
- **Rollback used:** none.

## Set-fold fix (2026-09-23, spec §25 P19)

- **Cause:** `sets::fold_sets` assumed every part is needed once per set; a Kogake Prime Set purchase (Blueprint ×1, Gauntlet ×2, Boot ×2) left a phantom Gauntlet and Boot in stock. warframe.market's part detail carries `quantityInSet`, which the set cache now fetches once per part and stores in `sets_v2.json`; the fold divides by it.
- **Branch:** `fix-set-fold-quantities` (`693c934`, from main `8fbcf95`), one commit in `sets.rs` plus a test literal. Opus review: no Critical or Important; the Kogake shapes, an absent blueprint, duplicated part lines, a zero quantity and the quantity-1 identity all traced by hand; four hygiene minors deferred (keep fetching after a failed part; the root's implicit quantity 1; `.max(1)` written twice; a float `quantityInSet` is an error, so that root never folds).
- **Gate at `693c934`:** `utils` 15, `qf_core` 259, `qf-server` 4+3 (+15 integration), RPC 89/89, `pnpm build` clean, warnings unchanged.
- **Deploy:** user-run at 2026-09-23 05:21 UTC, empty deletion preview, server recompiled, container healthy; the live trader was restarted by the user at 05:23:07 UTC.
- **Live acceptance: pending.** The next purchase of a set that needs two of a part must record one stock row at the trade's platinum and no leftovers, with an `info` line naming the parts and quantities the first time that set is cached. The stale `cache/sets.json` on ockohome is inert.

## Helper-trade variant fix (2026-09-24, spec §25 P20)

- **Symptom:** the Archon Vitality stock row (id 36, bought 60 p on 2026-09-23 04:54 UTC) was stamped `NoSellers` on every sell pass, six times in the two hours before 03:11 UTC, while warframe.market listed 286 rank-10 sellers.
- **Cause:** the in-game trade log gives name and rank only; `resolve::resolved` stored `{"rank":10}`. Archon Vitality's listing has variants (`regular`, `atragraph`) and every order carries one; the trader filters orders by exact sub-type equality, so the sell pass matched nothing. Buy candidates come from `item_stats` (`rank=10;subtype=regular`) and were unaffected. Any helper-detected trade of a variant-bearing item (relics, Ayatan sculptures) had the same gap.
- **Fix:** `d53afce` on `fix-helper-trade-variant`, one function: when the cache lists variants and the trade names none, the sub-type takes the first listed variant (`regular` for mods, `intact` for relics) beside the capped rank. Two tests added; gate at `d53afce`: `utils` 3, `qf_core` 261, `qf-server` 4+15, RPC 89/89, `pnpm build` clean, warnings unchanged. Main fast-forwarded to `d53afce`.
- **Deploy:** user-run at 2026-09-24 03:31 UTC, container healthy; row 36 set to `{"rank":10,"variant":"regular"}` by hand (status reset to `pending`); the user pressed Start at 03:35:17 UTC.
- **Live acceptance: PASS.** First cycle, 03:35:23 UTC: Archon Vitality processed as one entry (`Buy`, `Sell`), 4 buy and 18 sell orders seen, `Status: Live`, sell order `6ab49a7c9703a8846ca3ad81` created at 100 p. Purchase transaction 314 keeps its recorded `{"rank":10}`.
- **Noted, not fixed:** on container start the server tries to force the warframe.market status to invisible (`[Startup] Could not force invisible status: NotConnected` at 03:31:12) — the likely source of the status flip seen after the 2026-09-23 05:23 Start, rather than a click in the user menu.
