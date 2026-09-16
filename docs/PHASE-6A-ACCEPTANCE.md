# Phase 6a acceptance — trading analytics accepted (deployed 2026-09-16 17:50 UTC; checks run after)

- **Server:** `ockohome`, stack at `~/stacks/quantframe-server`, branch `phase-6a-trading-analytics` (`3c338a9`, merge base with main `0e0d3f3`). Web UI origin `http://quantframe.cgcorp.internal`.
- **Local gate (at `3c338a9`):**
  - Tests pass: `utils` 3, `qf_core` 196, `qf-server` 19 (4 unit, 15 integration).
  - `83 server commands, 83 used by web, 0 missing`. Phase 5 had 79; the four new ones are the `analytics_*` commands.
  - `pnpm build` (tsc and vite) is clean, built in 684 ms. Only pre-existing `unused import` warnings; nothing new.
- **Deploy:**
  - The user ran the deploy with `!` commands. The Claude Code auto-mode classifier denies an agent `rsync` outright, so the agent only verified afterwards.
  - The `rsync` now carries `--exclude 'crates/*/logs'` (phase 5 follow-up 3). It made **no deletions**. The full tree was listed as transferred because the worktree is a fresh checkout.
  - `docker compose config -q && docker compose up -d --build` brought the container to `Up 20 seconds (healthy)`.
  - `backups/`, `secrets/` and `.env` were excluded from the sync.
- **Boot log (2026-09-16 17:50 UTC):** `[Db:Connect] Database ready` 17:50:07, `Loaded 3840 tradable items` 17:50:07, `[Collector] Started: 3840 items, 24 hot` 17:50:09, `[Housekeeping] Started: tick 60 s, backups in /backups` 17:50:09, `[Startup] Core started` 17:50:09, `listening on 0.0.0.0:8080`. No panic, no `CRITICAL`, no `Trader started`. The first hourly maintenance line came at 17:50:24 with 299795 hourly rows and 7958 daily rows.
- ockohome now runs `3c338a9`, which includes all of phase 5. The phase 5 flip is still pending, on or after 2026-09-22.

| # | Check (spec §22 A9) | Result | Notes |
|---|---|---|---|
| 1 | Items P&L totals match the Transaction tab's financial report | Pass | Over a range ending on a no-trade day, revenue, expenses and profit equal the financial report's numbers |
| 2 | Galvanized Shot shows 3 purchases and 2 sales | Pass | Both counts as expected |
| 3 | Stock performance lists the 16 stock rows with market fields for tracked items | Pass | Tracked rows show a median; untracked and no-trade rows show "Not tracked" and "Warming up" with no fabricated loss |
| 4 | Trading partners counts are correct for a spot-checked user | Pass | One partner was spot-checked and its counts match the Transaction tab |
| 5 | The timeline's monthly totals match the Home page's yearly bar chart | Pass | Week buckets summed per month match the Home chart; day buckets show bars plus the cumulative line |

All five checks were confirmed by the user in the browser.

## Rulings made during execution

- Task 6's deploy is run by the user with `!` commands, because the auto-mode classifier denies an agent `rsync` outright, as learned in phase 5. An agent only verifies afterwards. Cost if wrong: none.
- Bare `wfm_id` and `item_name` under `GROUP BY (wfm_url, sub_type)` are fine. Both are functionally dependent on `wfm_url`. Cost if wrong: a display name.
- `avg_days_held` deliberately looks back to purchases before the range, per spec A2, "most recent purchase at or before the sale". It measures real holding time, not an in-range artefact. Cost if wrong: none.
- The reviewer's Important 1 and 2 on Task 1 stand: the plan-mandated `MAX(created_at)` text compare is wrong. The fix uses `julianday`, so `avg_days_held` uses `MAX(julianday(p.created_at))` and `last_trade_at` returns `datetime(MAX(julianday(created_at)))`, normalised to `YYYY-MM-DD HH:MM:SS`, which is fine for display. Minor 3, the in-range space-shaped fixture, was folded into the same round. Cost if wrong: `last_trade_at` loses sub-second text, which nobody reads.
- `allowlist_has_no_removed_features` banned the substring "analytics", because spec §10 cut the QF-API-backed analytics pages. Spec §22 now adds local-SQL `analytics_*` RPCs, so dropping that one word from the ban list is correct, with the reviewer confirming the other bans are intact. Cost if wrong: a guard test loses one word.
- The separate commit `36f6f9e`, which fixes the pre-existing flake `events::tests::emitted_frames_reach_subscribers` (phase 5 follow-up 1, the same `try_recv`/`find` pattern as its sibling), is accepted in this phase. Cost if wrong: none.
- The `items.search` string is passed as `SearchField`'s `description`, because the component has no label or placeholder props. Acceptable. Cost if wrong: a caption.
- The Timeline shows five totals (revenue, expenses, profit, sales, purchases) rather than spec A5's three. Kept, because the two extra cards are cheap and useful. Cost if wrong: two cards.
- The Stock warm badge is green, yellow and gray, three states, rather than Price History's green and gray. Kept.

## Follow-ups

1. **Move `sortRows` and `num` out of `Tabs/Items`** into `Tabs/sort.ts` or a shared util, when 6b needs them.
2. **End-date semantics differ.** The Transaction tab's financial report filters `created_at <= end date` (midnight), while analytics uses end plus one day, exclusive. Either align the Transaction tab or document the difference.
3. **No web test runner exists**, so `sortRows` is untested.
4. **`avg_days_held`'s correlated subquery has no index on `transaction`.** Fine at the current scale.
5. **The Transaction tab's own `DatePickerInput` still has `clearable`.** Left alone.
6. **The Timeline shows five totals rather than spec A5's three.** Kept.
7. **The Stock badge is three-state, green, yellow and gray.** Kept.
8. **Deferred minors from the task reviews.** The final review triaged all of them as "stays deferred". Task 1: the `avg_days_held` correlated subquery rescans `transaction` per sale, with no index on that table; `user_name <> ''` does not exclude a NULL `user_name`, which is moot because the column is NOT NULL DEFAULT `''`. Task 2: unrealised ties fall back on SQLite row order, so add `ORDER BY item_name` if a stable order is needed; `serde_json::from_str(..).ok()` on a malformed stored `sub_type` silently reads as "no sub type", which is plan-mandated and the column is app-written. Task 4: no web test runner exists, so `sortRows` is untested; `sortRows` lives in a component module and leaks through the `Tabs` barrel, its natural home being `Tabs/sort.ts` later; zero profit renders green, which the brief mandated as `< 0 ? red : green`. Task 5: the Timeline's `sum()` recomputes per render, over trivial row counts.
9. **Still open from `docs/PHASE-5-ACCEPTANCE.md`**, carried forward by number. Follow-up 1 there, the flaky events test, is closed by `36f6f9e` on this branch. Follow-up 3, the `crates/*/logs` rsync exclusion, is closed by the deploy command.
   - 2. Deferred minors from the phase 5 task reviews.
   - 4. The Log tab's `useTranslate` helpers are named like hooks but are plain functions.
   - 5. The list still open from `docs/PHASE-4D-ACCEPTANCE.md`, carried forward there by number.
   - 6. `auto_delete` is turned off before the flip, so the first live start adopts the existing real orders. The decision is made; the change itself happens at the flip.
