# Sell-Slot Priority, Profit Ranking and Book Prefetch — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan with Opus implementers and reviewers. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sales never wait for an order slot, buy candidates are chosen by expected profit, and the trader cycle roughly halves by overlapping book fetches — all under the P22 request budget.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md` §25 **P25–P27** (read them first); P22 for the budget, P24 for change-only writes.

**Evidence:** live 2026-09-28 23:11–00:01 UTC — ~39 order-limit skips per cycle (36 sell-side in 50 min), cycle ~160 s, 1.16 req/s of 2.5; the volume-only ranking excluded Longbow Sharpshot, Exodia Epidemic and Arcane Reaper.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-trader-improve`, branch `trader-slots-ranking-prefetch` (from `main`). Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target`.
- **Gate after every task:** `cargo test -p utils --lib && cargo test -p qf_core --lib && cargo test -p qf-server && python3 scripts/check-rpc-commands.py && (cd web && pnpm build)`; never bare `--workspace`. Output pristine; no new warnings.
- **Exact values:** `SELL_SLOT_MARGIN = 5`; delete reason `SellSlot`; unprofiled buy order counts as `potential_profit = 0`; `candidate_ranking` values `expected_profit` (default) | `volume`; `RANK_VOLUME_CAP = 50.0`; `BOOK_PREFETCH = 1` (at most two book fetches outstanding).
- Do not change the breaker, the limiter, the collector, or P24's change-only writes. `web/public/lang/en.json` by targeted insertion only.
- **Commits:** conventional commits ending with exactly `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`; do not push; never touch `main`.

---

### Task 1: Sell orders get first claim on order slots (P25)

**Files:** `crates/qf_core/src/trader/orders.rs` (side-aware `can_create_order`, a `lowest_profit_buy_order()` helper), `crates/qf_core/src/trader/helpers.rs` (`progress_order` Create branch around the "has reached the order limit" skip at ~line 373), `crates/qf_core/src/trader/item.rs` (`process_items`: compute `sell_backlog` once per cycle and pass it via `TradeContext` or `ItemTrader` state), tests in those files.

- [ ] **Step 1: Failing tests:** `buy_create_respects_the_sell_reserve` (limit 10, 5 orders, backlog 3 → buy allowed at 1 free-after-reserve slot, blocked at 2; sell allowed until 10); `a_blocked_sell_deletes_the_lowest_profit_buy_then_creates` (three buys with potential_profit 30/10/unset → the unset one is deleted with reason `SellSlot`, then the sell is created); `a_blocked_sell_with_no_buy_orders_is_skipped`; `dry_run_is_not_limited`. RED.
- [ ] **Step 2: Implement** per P25. The live limit comes from `client.order().get_order_limit()` and `total_orders()`; keep the existing skip log when a sell still cannot be placed. GREEN.
- [ ] **Step 3: Gate. Step 4: Commit** `feat(trader): reserve order slots for sales and free one when a sale is blocked`.

---

### Task 2: Rank buy candidates by expected profit (P26)

**Files:** `crates/qf_core/src/app/types/settings/item_wtb_settings.rs` (`candidate_ranking: String` or a small enum with serde, default `expected_profit`), `crates/qf_core/src/trader/price_source.rs` (`get_interesting_items` sort; a pure `rank_key(item, ranking) -> f64`; a pure `compare_rankings(items) -> RankingComparison { candidates, entered, left, expected_profit_new, expected_profit_old }`), the trader start path (one `info` line, component `Trader:Ranking`), the web settings form (a select beside Max Buy Candidates), `web/src/types/tauri.type.ts`, `en.json`.

- [ ] **Step 1: Failing tests:** `expected_profit_ranking_admits_a_high_spread_item` (fixture: 200/day × 5 p vs 20/day × 60 p, limit 1); `volume_ranking_is_unchanged`; `volume_is_capped_at_50_in_the_rank_key`; `ties_break_by_volume_then_uuid`; `pre_p26_settings_load_expected_profit`; `comparison_counts_entries_and_profit`. RED.
- [ ] **Step 2: Implement** per P26. GREEN.
- [ ] **Step 3: Gate. Step 4: Commit** `feat(trader): rank buy candidates by expected profit with a volume fallback`.

---

### Task 3: Two order books in flight (P27)

**Files:** `crates/qf_core/src/trader/item.rs` (`process_items`: start the next entry's `load_orders` future before processing the current entry; await it at the next iteration; drop it on stop), tests with a fake order source that counts overlap (extend the existing `fake_orders`/test harness rather than adding a new server).

- [ ] **Step 1: Failing tests:** `at_most_two_book_fetches_outstanding`; `entries_are_processed_in_the_same_order`; `a_prefetch_error_surfaces_at_its_own_entry`; `stop_drops_the_prefetch`. RED.
- [ ] **Step 2: Implement** per P27 (no new crate; `futures`/`tokio` already in the tree; no `tokio::spawn` needed if the future borrows `ctx` — keep it on the same task). GREEN.
- [ ] **Step 3: Gate. Step 4: Commit** `perf(trader): fetch the next order book while processing the current item`.

---

## Rollout (controller + user)

1. User deploys the branch tip (rsync from the worktree + compose, as before); trader restarts stopped.
2. Dry-run 1 h: the `Trader:Ranking` line (entered/left, expected profit old vs new); cycle length (expect ≈ 90 s); total ≤ 2.5 req/s; no order-limit skips on the sell side in dry-run is meaningless (no limit), so check the reserve on the live hour.
3. Live 1 h: sell-side "reached the order limit" skips ≈ 0; `SellSlot` deletes present when stock arrives at the cap; total ≤ 2.5 req/s; trades applied. Record in `docs/PHASE-5-ACCEPTANCE.md`, gate, ff-merge, cleanup.
