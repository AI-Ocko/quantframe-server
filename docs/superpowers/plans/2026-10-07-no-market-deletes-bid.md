# An Empty Market Deletes Our Buy Order — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan with an Opus implementer and a reviewer. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When `progress_buying` finds no other in-game buyer or no in-game seller, it deletes this item's existing buy order instead of leaving it standing unchecked.

**Architecture:** The early return at `crates/qf_core/src/trader/item.rs:413-416` calls the existing `delete_order` helper before returning, the same way the max-stock branch does at `item.rs:425`. `delete_order` (`crates/qf_core/src/trader/helpers.rs:420`) takes the reason operation as a parameter instead of hard-coding `MaxStock`.

**Tech Stack:** Rust; the existing `progress_buying` test harness in the `tests` module of `item.rs` (`ctx_with`, `book`, `seed_order`, `entry`, `price`, `route_for`, `ctx.orders.dry_log()`).

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md` §25 **P30** (read it first); §C4–C6 for the dry-run book, the max-stock delete and the failure counter.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-stale-bid`, branch `fix-stale-bid-no-volume`, stacked on `fix-backup-own-connection` (8dd794b) so both deploy together. Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target`.
- **Tests:** `cargo test -p utils --lib`, `cargo test -p qf_core --lib`, `cargo test -p qf-server`; never bare `--workspace`. Web: `python3 scripts/check-rpc-commands.py` and `(cd web && pnpm build)` (no web change is expected; the gate still runs). Output pristine; no new warnings (baseline 44).
- **Only these files may change:** `crates/qf_core/src/trader/item.rs`, `crates/qf_core/src/trader/helpers.rs`.
- **Unchanged behaviour:** the empty-market condition, its log line, every other buy rule, the sell and wish-list paths.
- **No network:** nothing in this change talks to warframe.market; do not run any binary, example or test outside the gate.
- **Commits:** conventional commits ending with exactly `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; do not push; never touch `main` or `fix-backup-own-connection`.

## Review Focus

1. An existing buy order and an empty buy side → deleted, reason contains `NoMarket`, no buy order left in the book (Step 1).
2. An existing buy order and an empty sell side → same (Step 1).
3. Empty market, no order → no write at all (Step 1).
4. The max-stock delete still carries `MaxStock` (Step 4).
5. A failed delete propagates exactly as the max-stock one does (same `map_err` chain); nothing new swallows an error.

---

### Task 1: The empty-market branch deletes the existing buy order

**Files:**
- Modify: `crates/qf_core/src/trader/helpers.rs` (`delete_order` signature and doc comment)
- Modify: `crates/qf_core/src/trader/item.rs` (the empty-market branch, the max-stock call, tests)

- [ ] **Step 1: Write the failing tests** in the `tests` module of `item.rs`, next to `max_stock_quantity_deletes_the_existing_buy_order`:

```rust
    #[tokio::test]
    async fn an_empty_market_deletes_the_existing_buy_order() {
        // Empty buy side, then empty sell side (spec P30).
        for live in [book(&[20, 25], &[]), book(&[], &[15, 17])] {
            let (_dir, ctx) = ctx_with(true, |_| {}).await;
            let route = route_for(true, true);
            seed_order(&ctx, OrderType::Buy, 17, route).await;
            let mut e = entry("Buy", None, None);
            e.apply_market_info(&live);
            progress_buying(&ctx, &item_info(), &mut e, &price(30.0, true), &live, route, &Slots::default()).await.unwrap();
            let last = ctx.orders.dry_log().last().unwrap().clone();
            assert_eq!((last.action.as_str(), last.side.as_str()), ("delete", "buy"));
            assert!(last.reason.contains("NoMarket"), "reason was {}", last.reason);
            assert!(ctx.orders.cache_orders().buy_orders.is_empty());
        }
    }

    #[tokio::test]
    async fn an_empty_market_without_a_buy_order_writes_nothing() {
        let (_dir, ctx) = ctx_with(true, |_| {}).await;
        let live = book(&[20, 25], &[]);
        let mut e = entry("Buy", None, None);
        e.apply_market_info(&live);
        progress_buying(&ctx, &item_info(), &mut e, &price(30.0, true), &live, route_for(true, true), &Slots::default()).await.unwrap();
        assert!(ctx.orders.dry_log().is_empty());
    }
```

- [ ] **Step 2: Run them and see the first one fail** (`cargo test -p qf_core --lib an_empty_market`): no `delete` is written, so the last log entry is not a delete (or the order is still in the book). The second one passes already; it guards the no-order case.

- [ ] **Step 3: Implement.**
  - In `helpers.rs`, give `delete_order` a final parameter `reason: &str` and build its operations as `OperationSet::from(vec!["Update", "Delete", reason])`. Update its doc comment to say the caller names the reason (amendment C5, spec P30).
  - In `item.rs`, the max-stock call at line ~425 passes `"MaxStock"`.
  - In `item.rs`, the empty-market branch becomes:

```rust
    if entry.buy_market_info.volume == 0 || entry.sell_market_info.volume == 0 {
        log(&format!("Item {} has no market volume. Skipping WTB order creation.", item_info.name));
        // Spec P30: an empty market must not leave our bid standing unchecked.
        delete_order(&component, entry, OrderType::Buy, &ctx.orders, route, "NoMarket")
            .await
            .map_err(|e| e.with_location(get_location!()).with_context(entry.to_json()))?;
        return Ok(());
    }
```

- [ ] **Step 4: Pin the max-stock tag.** In `max_stock_quantity_deletes_the_existing_buy_order`, add `assert!(ctx.orders.dry_log().last().unwrap().reason.contains("MaxStock"));` after the existing `action` assertion.

- [ ] **Step 5: Run the full gate** from the worktree:

```bash
export CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target
cargo test -p utils --lib && cargo test -p qf_core --lib && cargo test -p qf-server && python3 scripts/check-rpc-commands.py && (cd web && pnpm build)
```

Expected: all green; qf_core has 2 more tests than the branch base (342 → 344); warnings unchanged.

- [ ] **Step 6: Commit** only the two source files:

```bash
git add crates/qf_core/src/trader/item.rs crates/qf_core/src/trader/helpers.rs
git commit -m "fix(trader): delete our buy order when the market is empty

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
