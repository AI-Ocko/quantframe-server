# Phase 6b acceptance — market data accepted (deployed 2026-09-16 18:52 UTC; checks run after)

- **Server:** `ockohome`, stack at `~/stacks/quantframe-server`, branch `phase-6b-market-data` (`c12ef88`, merge base with main `4dfa935`). Web UI origin `http://quantframe.cgcorp.internal`.
- **Local gate (at `c12ef88`):**
  - Tests pass: `utils` 3, `qf_core` 201, `qf-server` 19 (4 unit, 15 integration).
  - `86 server commands, 86 used by web, 0 missing`. Phase 6a had 83. The three new ones are `market_overview`, `market_movers` and `market_warmup`.
  - `pnpm build` is clean, built in 547 ms. Only pre-existing `unused import` warnings.
- **Deploy:**
  - The user ran the deploy with `!` commands.
  - The `rsync` carried `--exclude 'crates/*/logs'` and made **no deletions**.
  - `docker compose config -q && docker compose up -d --build` brought the container to `Up 20 seconds (healthy)`.
  - `backups/`, `secrets/` and `.env` were excluded from the sync.
- **Boot log (2026-09-16 18:52 UTC):** `[Db:Connect] Database ready` 18:52:33, `Loaded 3840 tradable items` 18:52:34, `[Collector] Started: 3840 items, 25 hot` 18:52:36, `[Housekeeping] Started: tick 60 s, backups in /backups` 18:52:36, `[Startup] Core started` 18:52:36, `listening on 0.0.0.0:8080`. No panic, no `CRITICAL`, no `Trader started`.
- ockohome now runs `c12ef88`, which includes phases 5, 6a and 6b. The phase 5 flip is still pending, on or after 2026-09-22.

| # | Check (spec §23 M8) | Result | Notes |
|---|---|---|---|
| 1 | The overview lists about as many rows as the collector's item count, sorting puts the widest spreads first, and clicking a row lands on Price History with that item selected | Pass | All three behaved as expected in the browser |
| 2 | Movers show plausible items with the change matching Price History for one spot-checked item | Pass | A spot-checked riser's Was and Now match its Price History daily medians |
| 3 | Warm-up shows `warm = 0` before 2026-09-22 with a non-zero projection for the 22nd | Pass | Warm is 0 now, the projection bar is non-zero on or after 2026-09-22, and both histograms render |
| 4 | Price History for a busy item shows recent trades inside the daily min/max and a book whose top sell is at or above the hourly chart's latest min-sell | Pass | The latest order book and recent probable trades are shown, prices sit inside the daily min/max, and the top sell is at or above the latest hourly min-sell |

All four checks were confirmed by the user in the browser.

## Rulings made during execution

- Task 4 moves `sortRows` and `num` to a shared `web/src/utils/sortRows.ts`, imported by the 6a tabs and the new Overview tab, instead of duplicating the helper. This closes phase 6a follow-up 1. Cost if wrong: a small import churn in four files.
- Task 6's deploy is run by the user with `!` commands. Cost if wrong: none.
- Task 3's history test gained `recompute_item_stats(..)` in setup plus `assert_eq!(history.sub_type, "rank=0")`, because `load_history` picks its sub-type from `item_stats` ∪ `sweep_summary_hourly` and the brief's fixture wrote neither. Accepted. The brief's test was silently vacuous. Cost if wrong: none.
- The reviewer's Important 1 and 2 on Task 4 stand, both plan-mandated. Selection becomes consume-on-read (`takeSelection`), applied without the slug comparison, and `refetchInterval` is removed from Overview and Warm-up, which is consistent with `3c338a9`. Minor 3, the Warm-up loader, was folded into the same round. Cost if wrong: none.
- Spec M5 puts the market SQL in `collector/store.rs`. The plan and the code put it in the new `collector/market.rs` for better cohesion. Accepted, recorded here. Cost if wrong: a file location.
- The `Deserialize` derives on the market structs are house style, the same as `DryRunEntry`. Left as is.

## Follow-ups

1. **The Movers period labels read "24 h" and "7 d"**, but a stale item's comparison spans whatever gap exists. This is spec-inherent. Consider a §23 amendment to "vs previous day" and "vs previous week".
2. **A freshly swept item shows an empty book** until a stats recompute or hourly rollup runs, because `load_history`'s sub-type list excludes `sweep_summary`.
3. **The movers test pins one item per side.** The 25 cap and the within-side order are unpinned.
4. **The `> 0.0` filter on `median_then`** both omits "no comparison" and guards the division. One comment would prevent a future simplification.
5. **Two sort helpers live in `web/src/utils`**, `sorting.helper.ts` and `sortRows.ts`. Add a doc comment on each.
6. **`data.book!` non-null assertions in PriceHistory.** A local `const book = data.book` would avoid them.
7. **The overview payload is about 2.5 to 3 MB on ockohome**, where the spec guessed "well under 2 MB". That is fine on the LAN. Revisit if the item count grows.
8. **Deferred minors from the task reviews.** The final review triaged all of them as "stays deferred". Task 2: movers runs two queries, each with a correlated per-row subquery, which could fold into one if `item_stats_daily` grows; the `> 0.0` filter comment (follow-up 4); the one-item-per-side test (follow-up 3). Task 3: the empty book right after a first sweep (follow-up 2). Task 4: the page is not clamped when a refetch shrinks the result set, which is moot now that polling is gone; "{{shown}} of {{total}} items" counts filtered rows, not the page; the two sort helpers (follow-up 5). Task 5: the `data.book!` assertions (follow-up 6). The "vs previous day" label change is a §23 amendment candidate, not a fix.
9. **Still open from `docs/PHASE-6A-ACCEPTANCE.md`**, carried forward by number. Follow-up 1 there, the `sortRows` relocation, is closed by this phase.
   - 2. End-date semantics differ between the Transaction tab's financial report and analytics.
   - 3. No web test runner exists, so `sortRows` is untested.
   - 4. `avg_days_held`'s correlated subquery has no index on `transaction`.
   - 5. The Transaction tab's own `DatePickerInput` still has `clearable`.
   - 6. The Timeline shows five totals rather than spec A5's three.
   - 7. The Stock badge is three-state, green, yellow and gray.
   - 8. Deferred minors from the phase 6a task reviews.
   - 9. The list still open from `docs/PHASE-5-ACCEPTANCE.md`, carried forward there by number.
