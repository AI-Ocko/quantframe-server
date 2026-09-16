# Phase 6c acceptance — market history backfill accepted (deployed 2026-09-16 19:55 UTC; import finished 21:08 UTC)

- **Server:** `ockohome`, stack at `~/stacks/quantframe-server`, branch `phase-6c-market-backfill` (`df0bd8d`, merge base with main `dc9f434`). Web UI origin `http://quantframe.cgcorp.internal`.
- **Local gate (at `df0bd8d`):**
  - Tests pass: `utils` 3, `qf_core` 209 (7 new backfill tests), `qf-server` 19 (4 unit, 15 integration).
  - `88 server commands, 88 used by web, 0 missing`. Phase 6b had 86. The two new ones are `market_backfill_start` and `market_backfill_status`.
  - `pnpm build` is clean, built in 692 ms. Only pre-existing `unused import` warnings.
- **Deploy:**
  - The user ran the deploy with `!` commands.
  - The `rsync` carried `--exclude 'crates/*/logs'` and made **no deletions**.
  - `docker compose config -q && docker compose up -d --build` brought the container to `Up 20 seconds (healthy)`.
- **Boot log (2026-09-16 19:54:58 UTC):** `[Db:Connect] Database ready`, `Loaded 3840 tradable items`, then at 19:55:00 `[Collector] Started: 3840 items, 27 hot`, `[Housekeeping] Started`, `[Startup] Core started`, `listening on 0.0.0.0:8080`. No panic, no `CRITICAL`, no `Trader started`.
- **Import run:** the user pressed the button after the 19:55 deploy. The exact `Started` time was not captured. The finish line, verbatim: `[2026-09-16 21:08:33] [4416.4720] [INFO] [Backfill] Finished: items 3840, days 282495, missing 0, failed 0`. The elapsed stamp is time since boot, 73.6 minutes, so it is an upper bound on the run.
- ockohome now runs `df0bd8d`, which includes phases 5, 6a, 6b and 6c. The phase 5 flip is still pending, on or after 2026-09-22.

| # | Check (spec §24 K7) | Result | Notes |
|---|---|---|---|
| 1 | Price History for Ash Prime Set shows about 90 daily bars | Pass | Confirmed in the browser |
| 2 | Movers 7 d lists are populated | Pass | Both Rising and Falling have rows |
| 3 | Warm-up still shows `warm = 0` and the projection is unchanged | Pass | Warm is 0 and the projection is the same, which shows the trader's inputs were not touched |
| 4 | Pressing the button again finishes quickly with days added 0 | Pass, spec wording corrected | The second press re-fetches every item and adds 0 days, which is what K2 and K3 specify. K7's "finishes quickly" was wrong: a re-run costs the same 3 840 requests. Recorded as a spec wording error and follow-up 1, not a defect |

All four checks were confirmed by the user in the browser.

## Rulings made during execution

- The backfill's `fetch_with_retries` duplicates the retry and jitter rule from `fetch.rs`, six lines, instead of generalising `OrderSource`. Cost if wrong: one small duplicate to fold later.
- `insert_missing` does one autocommit INSERT per day, about 90 per item and several hundred thousand per run. Task 2 wraps each item's inserts in one transaction, using sea-orm's `TransactionTrait`: begin, insert loop, commit. Cost if wrong: none.
- The job test runs on the real clock with `jitter()` returning 0 under `cfg(test)`, because tokio's paused clock trips sqlx's pool acquire deadline. Accepted, and `fetch.rs` is untouched. Cost if wrong: a 0.06 s test.
- A panic in the spawned run would leave the status `Running` forever, so `start` spawns a wrapper that awaits the inner task's `JoinHandle` and on `Err` sets the state to `Failed` with `last_error` set to the panic message. This is also the spec's `failed` producer in K3. Cost if wrong: none.
- Task 2's three review minors were folded in before Task 3: `start()` returns a fully reset status, a concurrent-start test was added, and an empty item list ends `failed`. Cost if wrong: one small commit.
- Check 4 is recorded as a spec wording error, not a defect. A re-run costs the same 3 840 requests. The fix is to skip items that already hold 90 backfilled days, or to keep a "since" watermark. Cost if wrong: a second run wastes about 30 minutes of limiter time.

## Follow-ups

1. **A re-run re-fetches all 3 840 items.** Add a skip for items that already hold 90 backfilled days, and amend K7's wording.
2. **`.unwrap_or_default()` on the reqwest builder** silently drops the 30 s timeout if the build ever fails. `.expect` would surface it through `watch()`.
3. **Sub-type keys the collector never produces** (Ayatan amber and cyan stars, charges) collapse to `""` and are dead rows.
4. **The Movers "day" list may look noisy** across the collector and backfill boundary for a few days.
5. **During a run the Hot lane starves the collector's cold pass**, so the Collector tab's cold-lane numbers are stale for the duration. Expected, not a regression.
6. **One malformed row fails an item's whole 90 days.** The parse is all-or-nothing.
7. **Deferred minors from the task reviews.** The final review triaged all of them as "stays deferred". Task 2: `jitter()` computes randomness before discarding it under `cfg(test)`; per-item write transactions run against the collector's `rollup_daily` on the same table, which the final review closed as proven safe (the deferred transaction's first statement writes, and `rollup_daily` is autocommit per row); `run()`'s empty-list path takes two separate lock acquisitions, so a transient Running/0 is visible, which matches the file's per-field update pattern. Final fix wave: the reqwest builder's `.unwrap_or_default()` (follow-up 2).
8. **Still open from `docs/PHASE-6B-ACCEPTANCE.md`**, carried forward by number.
   - 1. The Movers period labels read "24 h" and "7 d", but a stale item's comparison spans whatever gap exists.
   - 2. A freshly swept item shows an empty book until a stats recompute or hourly rollup runs.
   - 3. The movers test pins one item per side; the 25 cap and the within-side order are unpinned.
   - 4. The `> 0.0` filter on `median_then` both omits "no comparison" and guards the division.
   - 5. Two sort helpers live in `web/src/utils`, `sorting.helper.ts` and `sortRows.ts`.
   - 6. `data.book!` non-null assertions in PriceHistory.
   - 7. The overview payload is about 2.5 to 3 MB on ockohome.
   - 8. Deferred minors from the phase 6b task reviews.
   - 9. The list still open from `docs/PHASE-6A-ACCEPTANCE.md`, carried forward there by number.
