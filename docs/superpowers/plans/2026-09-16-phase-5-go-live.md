# Phase 5: Go-live Prep — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task with Opus implementers and a reviewer between tasks. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Everything the go-live flip depends on, built and deployed while the trader stays in dry-run: honest simulated-delete log lines, a longer pause after idle cycles, a dry-run summary on the Dry-run log tab, and the go-live runbook. The flip itself happens on or after 2026-09-22 and is not part of these tasks.

**Architecture:** `TradeOrders::delete` reports the route it took so the trader's delete loop can word the line correctly. `ItemTrader::check` reports how many items it processed so `engine::run_loop` can sleep longer after an empty cycle, in 1 s slices that respect Stop. A new `trader_dry_run_summary` RPC runs two GROUP BY queries over `dry_run_log` and the Dry-run log tab renders them above the paged table. The runbook is a document.

**Tech Stack:** Rust (tokio, sea-orm raw SQL over SQLite), React 19 + Mantine + TanStack Query 5, pnpm 11.3.0.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md` — read **§21 (G1–G7)** first; §16 C4–C11 for the trader and dry-run design; §19 H1 for the `auto_delete` gate. §21 takes precedence.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-phase-5`, branch `phase-5-go-live` (from `main` at `0a8b114`). Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target`.
- **Tests:** `cargo test -p utils --lib`, `cargo test -p qf_core --lib`, `cargo test -p qf-server`; never bare `--workspace`. Web: `python3 scripts/check-rpc-commands.py` and `(cd web && pnpm build)`. Output pristine.
- **Exact strings:** log lines `Simulated delete of order <id> (<forced_by>) <i>/<total>` and `Deleted order with ID: <id> <i>/<total>`; constants `CYCLE_PAUSE` = 1 s and `IDLE_PAUSE` = 30 s in `engine.rs`; RPC `trader_dry_run_summary { days: i64 }`, `days` clamped to `1..=30`; `by_item` capped at 25 rows; en.json keys under `pages.live_scraper.trader.dry_run_log.summary`; runbook at `docs/GO-LIVE-RUNBOOK.md`.
- **Nothing in this plan turns global dry-run off** and nothing changes H1. A live start with `auto_delete` on stays refused.
- **`en.json`** by targeted insertion only (a Python script that loads, inserts under the existing key, dumps with `indent=2, ensure_ascii=False` and a trailing newline); confirm it parses afterwards and that the diff touches only the new keys.
- **Commits:** conventional commits ending with exactly `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`; `git push -u origin phase-5-go-live` after each task. Never touch `main`.
- Tasks 1–5 do not touch ockohome; Task 6 does (deploy + acceptance) and asks the user before deploying.

## File Structure (end of phase 5 prep)

```
crates/qf_core/src/trader/orders.rs              MOD  delete -> Result<Route, Error>; tests
crates/qf_core/src/trader/item.rs                MOD  check/process_items -> Result<usize, Error>; delete wording
crates/qf_core/src/trader/engine.rs              MOD  IDLE_PAUSE; run_loop(…, pause, idle_pause); tests
crates/qf_core/src/trader/platform.rs            MOD  pass IDLE_PAUSE
crates/qf_core/src/trader/store.rs               MOD  DryRunSummary, SummaryByAction, SummaryByItem, dry_run_summary; test
crates/qf_core/src/commands/trader.rs            MOD  trader_dry_run_summary
crates/qf_core/src/commands/rpc.rs               MOD  trader_dry_run_summary row + test
web/src/api/live_scraper/index.ts                MOD  dryRunSummary(days)
web/src/types/tauri.type.ts                      MOD  DryRunSummary types
web/src/pages/live_scraper/Tabs/DryRunLog/index.tsx  MOD  day selector + two summary tables
web/public/lang/en.json                          MOD  pages.live_scraper.trader.dry_run_log.summary.*
docs/GO-LIVE-RUNBOOK.md                          NEW
README.md                                        MOD  link to the runbook
docs/PHASE-5-ACCEPTANCE.md                       NEW  (Task 6)
```

---

### Task 1: `delete` reports its route and the trader says "Simulated"

**Files:**
- Modify: `crates/qf_core/src/trader/orders.rs:270-286` (`delete`), tests at the end of the file
- Modify: `crates/qf_core/src/trader/item.rs:53-80` (`delete_unwanted_orders`) and the knapsack delete near line 185

**Interfaces:**
- Consumes: `Route`, `ForcedBy`, `book_forced_by()` in `orders.rs`.
- Produces: `pub async fn delete(&self, order_id: &str, meta: &WriteMeta) -> Result<Route, Error>`.

- [ ] **Step 1: Write the failing tests** in the `tests` module of `orders.rs`, after `not_warm_items_are_simulated_when_global_dry_run_is_off`:

```rust
    #[tokio::test]
    async fn delete_reports_the_route_it_took() {
        let global = TradeOrders::new(None, None, true);
        let created = global.create(params("item1", OrderType::Buy, 17), route_for(true, true), &meta("Create")).await.unwrap();
        assert_eq!(global.delete(&created.id, &meta("AutoDelete")).await.unwrap(), Route::DryRun(ForcedBy::Global));
        // Under global dry-run an unknown id is still handled in the book (no live call), and reports Global.
        assert_eq!(global.delete("5f1c2d3e4a5b6c7d8e9f0a1b", &meta("AutoDelete")).await.unwrap(), Route::DryRun(ForcedBy::Global));

        let live_off = TradeOrders::new(None, None, false);
        let created = live_off.create(params("item2", OrderType::Sell, 40), route_for(false, false), &meta("Create")).await.unwrap();
        assert_eq!(live_off.delete(&created.id, &meta("Knapsack")).await.unwrap(), Route::DryRun(ForcedBy::NotWarm));
        // Not in the book and no live client: the live path is taken and fails as before.
        assert!(live_off.delete("5f1c2d3e4a5b6c7d8e9f0a1b", &meta("Knapsack")).await.is_err());
        assert_eq!(live_off.consecutive_failures(), 1);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib trader::orders::tests::delete_reports_the_route_it_took`
Expected: compile error, `()` is not `Route`.

- [ ] **Step 3: Change `delete`** in `orders.rs`:

```rust
    /// Deletes an order and reports whether it was a simulated or a real delete.
    pub async fn delete(&self, order_id: &str, meta: &WriteMeta) -> Result<Route, Error> {
        if self.in_book(order_id) {
            let order = {
                let mut book = self.book.lock().unwrap();
                let order = book.get_by_id(order_id);
                book.remove_by_id(order_id);
                order
            };
            let forced_by = self.book_forced_by();
            if let Some(order) = order {
                self.record("delete", &order, None, None, meta, forced_by).await;
            }
            return Ok(Route::DryRun(forced_by));
        }
        let client = self.live_client()?;
        let result = client.order().delete(order_id).await;
        self.finish_live("Delete", result).await.map(|_| Route::Live)
    }
```

- [ ] **Step 4: Word the trader's delete line** in `item.rs`, inside the `match ctx.orders.delete(id, &meta).await` of `delete_unwanted_orders`:

```rust
                Ok(Route::DryRun(forced_by)) => {
                    info(
                        comp("Delete"),
                        &format!("Simulated delete of order {} ({}) {}/{}", id, forced_by.as_str(), current_index, total),
                        &LoggerOptions::default(),
                    );
                    self.send_event("deleted", Some(json!({"current": current_index, "total": total, "id": id})));
                }
                Ok(Route::Live) => {
                    info(comp("Delete"), &format!("Deleted order with ID: {} {}/{}", id, current_index, total), &LoggerOptions::default());
                    self.send_event("deleted", Some(json!({"current": current_index, "total": total, "id": id})));
                }
```

`ForcedBy` is already imported in `item.rs` through `super::orders::{route_for, Route, WriteMeta}`; add `ForcedBy` only if the compiler asks. The knapsack branch (`if let Err(err) = ctx.orders.delete(&order.3, &meta).await`) needs no change: it ignores the `Ok` value.

- [ ] **Step 5: Run the trader tests**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib trader::`
Expected: all pass, including the new test. Fix any other caller of `delete` that matched on `Ok(())` (grep `orders.delete(` under `crates/qf_core/src`; the controller's stop path uses the wf-market client directly and is unaffected).

- [ ] **Step 6: Commit and push**

```bash
git add crates/qf_core/src/trader/orders.rs crates/qf_core/src/trader/item.rs
git commit -m "fix(trader): say 'Simulated delete' when the order was removed from the dry-run book

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push -u origin phase-5-go-live
```

---

### Task 2: Idle cycles pause 30 s, Stop still takes a second

**Files:**
- Modify: `crates/qf_core/src/trader/engine.rs` (`CYCLE_PAUSE`, `run_loop`, tests)
- Modify: `crates/qf_core/src/trader/item.rs:83-200` (`check`, `process_items`)
- Modify: `crates/qf_core/src/trader/platform.rs:59-77` (`spawn_engine`)

**Interfaces:**
- Consumes: `ItemTrader::check`, `engine::run_loop`.
- Produces: `pub const IDLE_PAUSE: Duration = Duration::from_secs(30);` and
  `pub async fn run_loop<F, Fut>(running, just_started, orders, check: F, pause: Duration, idle_pause: Duration) -> EngineExit where Fut: Future<Output = Result<usize, Error>>`;
  `ItemTrader::check(&self, ctx) -> Result<usize, Error>`.

- [ ] **Step 1: Write the failing tests** in `engine.rs`'s `tests` module. First update the three existing tests: every `Ok(())` in a check closure becomes `Ok(1)`, and every `run_loop(..., CYCLE_PAUSE)` call becomes `run_loop(..., CYCLE_PAUSE, IDLE_PAUSE)`. Then add:

```rust
    #[tokio::test(start_paused = true)]
    async fn an_empty_cycle_sleeps_idle_pause_and_a_busy_one_sleeps_pause() {
        let running = Arc::new(AtomicBool::new(true));
        let orders = Arc::new(TradeOrders::new(None, None, true));
        let calls = Arc::new(AtomicUsize::new(0));
        let started = tokio::time::Instant::now();
        let stamps = Arc::new(Mutex::new(Vec::new()));
        let check = {
            let (running, calls, stamps) = (running.clone(), calls.clone(), stamps.clone());
            move || {
                let (running, calls, stamps) = (running.clone(), calls.clone(), stamps.clone());
                async move {
                    stamps.lock().unwrap().push(started.elapsed());
                    let n = calls.fetch_add(1, Ordering::SeqCst);
                    if n == 2 {
                        running.store(false, Ordering::SeqCst);
                    }
                    Ok(if n == 0 { 0 } else { 3 })
                }
            }
        };
        let exit = run_loop(running, Arc::new(AtomicBool::new(false)), orders, check, CYCLE_PAUSE, IDLE_PAUSE).await;
        assert_eq!(exit, EngineExit::Stopped);
        let stamps = stamps.lock().unwrap().clone();
        // cycle 0 (empty) -> 30 s -> cycle 1 (busy) -> 1 s -> cycle 2
        assert_eq!(stamps.len(), 3);
        assert_eq!(stamps[1] - stamps[0], IDLE_PAUSE);
        assert_eq!(stamps[2] - stamps[1], CYCLE_PAUSE);
    }

    #[tokio::test(start_paused = true)]
    async fn stop_interrupts_the_idle_pause_within_a_second() {
        let running = Arc::new(AtomicBool::new(true));
        let orders = Arc::new(TradeOrders::new(None, None, true));
        let handle = tokio::spawn(run_loop(
            running.clone(),
            Arc::new(AtomicBool::new(false)),
            orders,
            || async { Ok(0) },
            CYCLE_PAUSE,
            IDLE_PAUSE,
        ));
        tokio::time::sleep(Duration::from_millis(1500)).await; // inside the first idle pause
        let before = tokio::time::Instant::now();
        running.store(false, Ordering::SeqCst);
        assert_eq!(handle.await.unwrap(), EngineExit::Stopped);
        assert!(before.elapsed() <= Duration::from_secs(1), "took {:?}", before.elapsed());
    }
```

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib trader::engine`
Expected: compile errors (`IDLE_PAUSE` missing, arity, `Ok(usize)` vs `()`).

- [ ] **Step 3: Implement in `engine.rs`**

```rust
pub const CYCLE_PAUSE: Duration = Duration::from_secs(1);
/// After a cycle that processed nothing (spec §21 G2). Slept in 1 s slices so Stop is not delayed.
pub const IDLE_PAUSE: Duration = Duration::from_secs(30);
```

and the loop:

```rust
/// Runs `check` cycles until `running` is cleared, a critical error occurs, or order calls keep failing.
/// `check` returns how many items the cycle processed; an empty cycle sleeps `idle_pause` instead of `pause`.
pub async fn run_loop<F, Fut>(
    running: Arc<AtomicBool>,
    just_started: Arc<AtomicBool>,
    orders: Arc<TradeOrders>,
    mut check: F,
    pause: Duration,
    idle_pause: Duration,
) -> EngineExit
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<usize, Error>>,
{
    just_started.store(true, Ordering::SeqCst);
    while running.load(Ordering::SeqCst) {
        let mut processed = None;
        match check().await {
            Ok(n) => processed = Some(n),
            Err(mut e) => {
                e.log_level = classify(&e);
                let _ = e.log(LOG_FILE);
                if matches!(e.log_level, LogLevel::Critical) {
                    running.store(false, Ordering::SeqCst);
                    return EngineExit::Critical(format!("{}: {}", e.component, e.message));
                }
            }
        }
        let failures = orders.consecutive_failures();
        if failures >= MAX_CONSECUTIVE_FAILURES {
            running.store(false, Ordering::SeqCst);
            return EngineExit::OrderFailures(failures);
        }
        let wanted = if processed == Some(0) { idle_pause } else { pause };
        sleep_while_running(&running, wanted).await;
        just_started.store(false, Ordering::SeqCst);
    }
    EngineExit::Stopped
}

/// Sleeps `total` in slices of at most 1 s, returning early once `running` is cleared.
async fn sleep_while_running(running: &AtomicBool, total: Duration) {
    let slice = Duration::from_secs(1);
    let mut left = total;
    while !left.is_zero() && running.load(Ordering::SeqCst) {
        let step = left.min(slice);
        tokio::time::sleep(step).await;
        left -= step;
    }
}
```

- [ ] **Step 4: Make `check` count** in `item.rs`:

```rust
    /// One trader cycle. Returns how many interesting items were processed (0 = idle cycle).
    pub async fn check(&self, ctx: &TradeContext) -> Result<usize, Error> {
        info(comp("Check"), "Checking items...", &LoggerOptions::default());
        let my_orders = ctx.orders.cache_orders();
        self.delete_unwanted_orders(ctx, &my_orders).await?;
        let interesting_items = collect_interesting_items(ctx, COMPONENT).await?;
        self.process_items(interesting_items, ctx).await
    }

    async fn process_items(&self, mut interesting_items: Vec<ItemEntry>, ctx: &TradeContext) -> Result<usize, Error> {
```

and at the end of `process_items` replace `Ok(())` with `Ok(total)`. Every `return Ok(())` earlier in that function (if any) becomes `return Ok(total)`; a `break` out of the loop still reaches the final `Ok(total)`.

- [ ] **Step 5: Pass the constant** in `platform.rs` `spawn_engine`:

```rust
            engine::run_loop(running, just_started, orders, check, engine::CYCLE_PAUSE, engine::IDLE_PAUSE).await
```

- [ ] **Step 6: Run the qf_core tests**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib`
Expected: all pass. If `trader::item` golden tests call `check`/`process_items` and assert on `()`, change those assertions to the count (`assert_eq!(…, n)` where `n` is the number of entries the test feeds).

- [ ] **Step 7: Commit and push**

```bash
git add crates/qf_core/src/trader/engine.rs crates/qf_core/src/trader/item.rs crates/qf_core/src/trader/platform.rs
git commit -m "perf(trader): pause 30 s after an idle cycle, in 1 s slices so Stop stays prompt

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 3: `trader_dry_run_summary` RPC

**Files:**
- Modify: `crates/qf_core/src/trader/store.rs` (types after `DryRunPage`, function after `dry_run_page`, test)
- Modify: `crates/qf_core/src/commands/trader.rs` (new command)
- Modify: `crates/qf_core/src/commands/rpc.rs:67` (row) and the `trader_commands_are_routable_and_validate_args` test

**Interfaces:**
- Consumes: `count`, `stmt`, `db_err`, `ts` helpers already imported in `store.rs`; `DATABASE` in `commands/trader.rs`.
- Produces:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SummaryByAction { pub action: String, pub side: String, pub forced_by: String, pub count: i64 }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SummaryByItem { pub item_id: String, pub sub_type: String, pub action: String, pub count: i64, pub min_price: Option<i64>, pub max_price: Option<i64> }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DryRunSummary { pub since: String, pub by_action: Vec<SummaryByAction>, pub by_item: Vec<SummaryByItem> }
pub async fn dry_run_summary(conn: &DatabaseConnection, since: DateTime<Utc>) -> Result<DryRunSummary, Error>;
pub async fn trader_dry_run_summary(days: i64) -> Result<DryRunSummary, Error>;   // commands/trader.rs
```

- [ ] **Step 1: Write the failing store test** in `store.rs` `tests`, after `dry_run_log_pages_newest_first_and_prunes_old_rows`:

```rust
    #[tokio::test]
    async fn dry_run_summary_groups_rows_since_a_cutoff_and_caps_items() {
        let (_dir, conn) = db().await;
        // Too old for the cutoff below.
        insert_dry_run(&conn, &entry("2026-09-01T00:00:00Z", "create")).await.unwrap();
        // Two creates and one delete for item1 (entry() uses item1/buy/global/price 17).
        insert_dry_run(&conn, &entry("2026-09-15T00:00:00Z", "create")).await.unwrap();
        let mut dearer = entry("2026-09-15T00:01:00Z", "create");
        dearer.price = Some(25);
        insert_dry_run(&conn, &dearer).await.unwrap();
        let mut deleted = entry("2026-09-15T00:02:00Z", "delete");
        deleted.price = None;
        insert_dry_run(&conn, &deleted).await.unwrap();
        // 30 more items with one create each, to push item1's create row past the cap only if ties sort badly.
        for i in 0..30 {
            let mut e = entry("2026-09-15T01:00:00Z", "create");
            e.item_id = format!("filler{i:02}");
            insert_dry_run(&conn, &e).await.unwrap();
        }

        let summary = dry_run_summary(&conn, parse_ts("2026-09-10T00:00:00Z").unwrap()).await.unwrap();
        assert_eq!(summary.since, "2026-09-10T00:00:00Z");
        assert_eq!(
            summary.by_action,
            vec![
                SummaryByAction { action: "create".into(), side: "buy".into(), forced_by: "global".into(), count: 32 },
                SummaryByAction { action: "delete".into(), side: "buy".into(), forced_by: "global".into(), count: 1 },
            ]
        );
        assert_eq!(summary.by_item.len(), 25, "capped at 25 rows");
        assert_eq!(
            summary.by_item[0],
            SummaryByItem { item_id: "item1".into(), sub_type: "rank=0".into(), action: "create".into(), count: 2, min_price: Some(17), max_price: Some(25) }
        );
        assert!(summary.by_item.iter().all(|row| row.count >= 1));
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib trader::store::tests::dry_run_summary`
Expected: compile error, `dry_run_summary` not found.

- [ ] **Step 3: Implement in `store.rs`**. Types after `DryRunPage`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SummaryByAction {
    pub action: String,
    pub side: String,
    pub forced_by: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SummaryByItem {
    pub item_id: String,
    pub sub_type: String,
    pub action: String,
    pub count: i64,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
}

/// Counts over `dry_run_log` rows at or after `since` (spec §21 G3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DryRunSummary {
    pub since: String,
    pub by_action: Vec<SummaryByAction>,
    pub by_item: Vec<SummaryByItem>,
}
```

Function after `dry_run_page`:

```rust
/// Two GROUP BYs over rows at or after `since`; `by_item` keeps the 25 busiest (item, sub_type, action) rows.
pub async fn dry_run_summary(conn: &DatabaseConnection, since: DateTime<Utc>) -> Result<DryRunSummary, Error> {
    const C: &str = "Trader:DryRunSummary";
    let since = ts(since);
    let by_action = conn
        .query_all(stmt(
            "SELECT action, side, forced_by, COUNT(*) AS count FROM dry_run_log
             WHERE at >= ? GROUP BY action, side, forced_by ORDER BY action, side, forced_by",
            vec![since.clone().into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            Ok(SummaryByAction {
                action: r.try_get("", "action").map_err(|e| db_err(C, e))?,
                side: r.try_get("", "side").map_err(|e| db_err(C, e))?,
                forced_by: r.try_get("", "forced_by").map_err(|e| db_err(C, e))?,
                count: r.try_get("", "count").map_err(|e| db_err(C, e))?,
            })
        })
        .collect::<Result<_, Error>>()?;
    let by_item = conn
        .query_all(stmt(
            "SELECT item_id, sub_type, action, COUNT(*) AS count, MIN(price) AS min_price, MAX(price) AS max_price
             FROM dry_run_log WHERE at >= ?
             GROUP BY item_id, sub_type, action ORDER BY count DESC, item_id, sub_type, action LIMIT 25",
            vec![since.clone().into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            Ok(SummaryByItem {
                item_id: r.try_get("", "item_id").map_err(|e| db_err(C, e))?,
                sub_type: r.try_get("", "sub_type").map_err(|e| db_err(C, e))?,
                action: r.try_get("", "action").map_err(|e| db_err(C, e))?,
                count: r.try_get("", "count").map_err(|e| db_err(C, e))?,
                min_price: r.try_get("", "min_price").map_err(|e| db_err(C, e))?,
                max_price: r.try_get("", "max_price").map_err(|e| db_err(C, e))?,
            })
        })
        .collect::<Result<_, Error>>()?;
    Ok(DryRunSummary { since, by_action, by_item })
}
```

If `try_get` on `MIN(price)` fails to decode a NULL into `Option<i64>` (SeaORM decodes NULL as `None` for `Option`; if it errors on the SQLite type, wrap as `CAST(MIN(price) AS INTEGER)`), fix it in the SQL, not by dropping the field.

- [ ] **Step 4: Run the store test**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib trader::store`
Expected: PASS.

- [ ] **Step 5: Add the command** in `commands/trader.rs` after `trader_dry_run_log` (extend the `use crate::trader::store::{…}` line with `DryRunSummary`):

```rust
/// Counts over the dry-run log for the last `days` (clamped to 1..=30, the log's retention).
pub async fn trader_dry_run_summary(days: i64) -> Result<DryRunSummary, Error> {
    let conn = DATABASE.get().ok_or_else(|| Error::new("Trader:Rpc", "Database is not ready", get_location!()))?;
    let since = Utc::now() - chrono::Duration::days(days.clamp(1, 30));
    store::dry_run_summary(conn, since).await
}
```

- [ ] **Step 6: Register the RPC** in `rpc.rs` right after the `trader_dry_run_log` row:

```rust
    trader_dry_run_summary => trader::trader_dry_run_summary { days: i64 },
```

and extend the test `trader_commands_are_routable_and_validate_args`: add `"trader_dry_run_summary"` to the `for name in [...]` list and, after the `limit is required` assertion:

```rust
        assert!(dispatch("trader_dry_run_summary", json!({})).await.unwrap().is_err(), "days is required");
```

- [ ] **Step 7: Run the command tests and the RPC script**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib commands:: && python3 scripts/check-rpc-commands.py`
Expected: tests pass; the script reports `79 server commands, 78 used by web, 1 missing` (the web side comes in Task 4). Do not "fix" that here.

- [ ] **Step 8: Commit and push**

```bash
git add crates/qf_core/src/trader/store.rs crates/qf_core/src/commands/trader.rs crates/qf_core/src/commands/rpc.rs
git commit -m "feat(trader): add trader_dry_run_summary counts over the dry-run log

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 4: Summary tables on the Dry-run log tab

**Files:**
- Modify: `web/src/api/live_scraper/index.ts` (after `dryRunLog`)
- Modify: `web/src/types/tauri.type.ts:1132-1137` (after `DryRunPage`)
- Modify: `web/src/pages/live_scraper/Tabs/DryRunLog/index.tsx`
- Modify: `web/public/lang/en.json` (`pages.live_scraper.trader.dry_run_log.summary`)

**Interfaces:**
- Consumes: RPC `trader_dry_run_summary { days }` → `{ since, by_action, by_item }` from Task 3; the existing `cache_items` query and `names` map in the tab.
- Produces: `api.live_scraper.dryRunSummary(days: number): Promise<TauriTypes.DryRunSummary>`.

- [ ] **Step 1: Types** in `tauri.type.ts` directly after `DryRunPage`:

```ts
  export interface DryRunSummaryByAction {
    action: "create" | "update" | "delete";
    side: "buy" | "sell";
    forced_by: "global" | "not_warm";
    count: number;
  }
  export interface DryRunSummaryByItem {
    item_id: string;
    sub_type: string;
    action: "create" | "update" | "delete";
    count: number;
    min_price?: number | null;
    max_price?: number | null;
  }
  export interface DryRunSummary {
    since: string;
    by_action: DryRunSummaryByAction[];
    by_item: DryRunSummaryByItem[];
  }
```

- [ ] **Step 2: API method** in `web/src/api/live_scraper/index.ts` after `dryRunLog`:

```ts
  dryRunSummary(days: number) {
    return this.client.sendInvoke<TauriTypes.DryRunSummary>("trader_dry_run_summary", { days });
  }
```

- [ ] **Step 3: Strings** — insert with a script (targeted, no wholesale rewrite):

```bash
cd web && python3 - <<'EOF'
import json
p = "public/lang/en.json"
d = json.load(open(p, encoding="utf-8"))
node = d["pages"]["live_scraper"]["trader"]["dry_run_log"]
assert "summary" not in node
node["summary"] = {
  "title": "Summary",
  "days": "Last {{days}} days",
  "since": "Since {{since}}",
  "by_action": "By action",
  "by_item": "Busiest items",
  "action": "Action",
  "side": "Side",
  "forced_by": "Forced by",
  "count": "Count",
  "item": "Item",
  "sub_type": "Rank / variant",
  "min_price": "Min price",
  "max_price": "Max price",
  "empty": "No simulated writes in this window."
}
with open(p, "w", encoding="utf-8") as f:
    json.dump(d, f, indent=2, ensure_ascii=False)
    f.write("\n")
EOF
python3 -c "import json; json.load(open('public/lang/en.json', encoding='utf-8'))" && git diff --stat public/lang/en.json
```

Expected: the diff is a single insertion block (about 16 added lines) under `dry_run_log`. If the diff touches other lines (re-serialisation changed formatting), revert and insert the block with a text edit instead.

- [ ] **Step 4: The tab**. Replace the body of `DryRunLog/index.tsx` with:

```tsx
import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Badge, Group, Pagination, SegmentedControl, Stack, Table, Text, Title } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";

const LIMIT = 50;
const DAY_OPTIONS = ["1", "7", "30"];

export function DryRunLogPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`live_scraper.trader.dry_run_log.${key}`, context);
  const [page, setPage] = useState(1);
  const [days, setDays] = useState("7");
  const { data } = useQuery({
    queryKey: ["trader_dry_run_log", page],
    queryFn: () => api.live_scraper.dryRunLog(page, LIMIT),
    refetchInterval: 10_000,
    enabled: !!isActive,
  });
  const { data: summary } = useQuery({
    queryKey: ["trader_dry_run_summary", days],
    queryFn: () => api.live_scraper.dryRunSummary(Number(days)),
    refetchInterval: 10_000,
    enabled: !!isActive,
  });
  const { data: items } = useQuery({ queryKey: ["cache_items"], queryFn: () => api.cache.getTradableItems() });
  const names = useMemo(() => new Map((items ?? []).map((item) => [item.wfmId, item.name])), [items]);
  const pages = Math.max(1, Math.ceil((data?.total ?? 0) / LIMIT));
  const actionColor = (action: string) => (action === "delete" ? "red" : action === "create" ? "green" : "blue");

  return (
    <Stack mt="md">
      <Group justify="space-between">
        <Title order={5}>{t("summary.title")}</Title>
        <SegmentedControl value={days} onChange={setDays} data={DAY_OPTIONS.map((value) => ({ value, label: t("summary.days", { days: value }) }))} />
      </Group>
      {summary && (
        <Text size="sm" c="dimmed">
          {t("summary.since", { since: summary.since })}
        </Text>
      )}
      {summary && summary.by_action.length === 0 ? (
        <Text size="sm" c="dimmed">
          {t("summary.empty")}
        </Text>
      ) : (
        <Group align="flex-start" grow>
          <Table striped withTableBorder>
            <Table.Caption>{t("summary.by_action")}</Table.Caption>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>{t("summary.action")}</Table.Th>
                <Table.Th>{t("summary.side")}</Table.Th>
                <Table.Th>{t("summary.forced_by")}</Table.Th>
                <Table.Th>{t("summary.count")}</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {(summary?.by_action ?? []).map((row) => (
                <Table.Tr key={`${row.action}-${row.side}-${row.forced_by}`}>
                  <Table.Td>
                    <Badge color={actionColor(row.action)}>{row.action}</Badge>
                  </Table.Td>
                  <Table.Td>{row.side}</Table.Td>
                  <Table.Td>{row.forced_by}</Table.Td>
                  <Table.Td>{row.count}</Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
          <Table striped withTableBorder>
            <Table.Caption>{t("summary.by_item")}</Table.Caption>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>{t("summary.item")}</Table.Th>
                <Table.Th>{t("summary.sub_type")}</Table.Th>
                <Table.Th>{t("summary.action")}</Table.Th>
                <Table.Th>{t("summary.count")}</Table.Th>
                <Table.Th>{t("summary.min_price")}</Table.Th>
                <Table.Th>{t("summary.max_price")}</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {(summary?.by_item ?? []).map((row) => (
                <Table.Tr key={`${row.item_id}-${row.sub_type}-${row.action}`}>
                  <Table.Td>{names.get(row.item_id) ?? row.item_id}</Table.Td>
                  <Table.Td>{row.sub_type || "—"}</Table.Td>
                  <Table.Td>
                    <Badge color={actionColor(row.action)}>{row.action}</Badge>
                  </Table.Td>
                  <Table.Td>{row.count}</Table.Td>
                  <Table.Td>{row.min_price ?? "—"}</Table.Td>
                  <Table.Td>{row.max_price ?? "—"}</Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
        </Group>
      )}
      <Text size="sm" c="dimmed">
        {t("total", { total: data?.total ?? 0 })}
      </Text>
      <Table striped withTableBorder>
        <Table.Thead>
          <Table.Tr>
            <Table.Th>{t("at")}</Table.Th>
            <Table.Th>{t("action")}</Table.Th>
            <Table.Th>{t("side")}</Table.Th>
            <Table.Th>{t("item")}</Table.Th>
            <Table.Th>{t("sub_type")}</Table.Th>
            <Table.Th>{t("price")}</Table.Th>
            <Table.Th>{t("quantity")}</Table.Th>
            <Table.Th>{t("forced_by")}</Table.Th>
            <Table.Th>{t("reason")}</Table.Th>
          </Table.Tr>
        </Table.Thead>
        <Table.Tbody>
          {(data?.results ?? []).map((entry) => (
            <Table.Tr key={entry.id}>
              <Table.Td>{entry.at}</Table.Td>
              <Table.Td>
                <Badge color={actionColor(entry.action)}>{entry.action}</Badge>
              </Table.Td>
              <Table.Td>{entry.side}</Table.Td>
              <Table.Td>{names.get(entry.item_id) ?? entry.item_id}</Table.Td>
              <Table.Td>{entry.sub_type || "—"}</Table.Td>
              <Table.Td>{entry.price ?? "—"}</Table.Td>
              <Table.Td>{entry.quantity ?? "—"}</Table.Td>
              <Table.Td>{entry.forced_by}</Table.Td>
              <Table.Td>{entry.reason}</Table.Td>
            </Table.Tr>
          ))}
        </Table.Tbody>
      </Table>
      <Pagination total={pages} value={page} onChange={setPage} />
    </Stack>
  );
}
```

If `Table.Caption` does not exist in the installed Mantine version (check `web/node_modules/@mantine/core/package.json` and the `Table` exports), replace each caption with a `<Text size="sm" fw={500}>` above the table inside a `<Stack gap="xs">`.

- [ ] **Step 5: Build and check the RPC map**

Run: `python3 scripts/check-rpc-commands.py && (cd web && pnpm build)`
Expected: `79 server commands, 79 used by web, 0 missing`; tsc and vite clean, no new warnings.

- [ ] **Step 6: Commit and push**

```bash
git add web/src/api/live_scraper/index.ts web/src/types/tauri.type.ts web/src/pages/live_scraper/Tabs/DryRunLog/index.tsx web/public/lang/en.json
git commit -m "feat(web): show dry-run summary counts with a 1/7/30-day selector on the Dry-run log tab

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 5: Go-live runbook

**Files:**
- Create: `docs/GO-LIVE-RUNBOOK.md`
- Modify: `README.md` (the bullet list in "Running on the homelab", after the **Restore** bullet)

**Interfaces:**
- Consumes: the UI as it exists after Tasks 1–4 (Trader panel, Dry-run log tab with summary, Settings pages, Market Data page, Log tab).
- Produces: the document the user follows on or after 2026-09-22.

- [ ] **Step 1: Verify the UI facts the runbook names** before writing, and use the exact labels you find:
  - The Trader panel's Dry-run toggle and the checklist row labels: `grep -n "auto_delete_off\|dry_run\|checklist" web/public/lang/en.json | head` and `web/src/pages/live_scraper/index.tsx` (or the Trader panel component it imports).
  - Where `auto_delete` lives in Settings: `grep -rn "auto_delete" web/src/pages/settings --include=*.tsx | head -3`.
  - Where the On Alert webhook lives: `grep -rn "on_alert" web/src/pages/settings --include=*.tsx | head -3`.
  - The Market Data page's warm indicator: `grep -rn "warm\|history_days" web/src/pages/market_data --include=*.tsx | head -5`.
  - The token-expiry surface: `grep -rn "expires\|token_expires" web/src --include=*.tsx | head -3`.

- [ ] **Step 2: Write `docs/GO-LIVE-RUNBOOK.md`** with exactly these sections and this content, substituting the labels from Step 1 where a placeholder in angle brackets appears:

```markdown
# Go-live runbook

Turning global dry-run off is the last step of phase 5 (spec §21). It is a manual, one-time action. Do not do it before **2026-09-22**: the spec requires at least 7 days of dry-run on the server (dry-run started 2026-09-15) and items only become `warm` (7 days of history and at least 10 probable trades in 7 days) from about that date. Before that, nothing routes live even with the flag off, and the dry-run log has nothing to review.

The trader routes per item: with global dry-run **off**, a warm item is traded live and a not-warm item is still simulated (`forced_by = not_warm`). So the Dry-run log keeps filling after the flip. That is expected.

## 1. Pre-flight (all must hold)

- [ ] It is 2026-09-22 or later.
- [ ] **Market Data** shows warm items: `<warm label from Step 1>` for the items you expect to trade.
- [ ] **Live Scraper → Dry-run log → Summary, last 7 days** shows `create` and `update` rows (not only `delete`), and their min/max prices are plausible next to the Market Data medians for the same items. The only `delete` rows should be `AutoDelete` on Start (see follow-up 2 in `docs/PHASE-4D-ACCEPTANCE.md`) and `Knapsack`; a delete count far above the create count means the settings need a look before going live.
- [ ] **Settings → Live Scraper → `<auto_delete label>`** is **off**. The Trader panel checklist row `<auto_delete_off label>` is green. (Decision 2026-09-16: the first live start adopts the existing real orders rather than deleting them. A live start is refused while it is on.)
- [ ] **Trader panel → Delete buy orders on stop** is set the way you want it for live (off keeps buy orders on warframe.market when the trader stops; on deletes them).
- [ ] **Settings → Notifications → On Alert** has the Discord webhook, and **On Trader Stopped** as well.
- [ ] Today's backup exists on ockohome: `ls ~/stacks/quantframe-server/backups/quantframe-$(date -u +%F).sqlite`.
- [ ] The helper is connected and Warframe is running (both checklist rows green). Keep them that way for the first hour.
- [ ] The warframe.market token does not expire within 7 days (`<token expiry surface from Step 1>`; there has been no "token expiring" alert).

## 2. The flip

1. Open **Live Scraper**. In the Trader panel turn **Dry-run** off. The badge must read **Ready**; if it reads Offline, a checklist row is red — fix it, do not force anything.
2. Press **Start**. Open the **Log** tab.
3. Within the first cycle you should see, in this order: `Trader started (live)`; the warframe.market status set to `ingame`; `Checking items...`; `Processing Item: … | Route: Live` for warm items and `Route: DryRun(NotWarm)` for the rest. No `Simulated delete` or `Deleted order with ID` lines are expected because `auto_delete` is off.
4. Open your warframe.market profile in another tab. The first live `create` or `update` from the log must be visible there within a minute. Note its order id for the acceptance record.

## 3. First hour

- Watch the Log tab for `Trader stopped`. The reason is in the Trader panel and on Discord (On Trader Stopped). `OrderFailures(5)` means five consecutive warframe.market write failures; `Critical` means a parsing or bad-request error. Either way: read the lines before it, fix the cause, and Start again.
- The Dry-run summary keeps growing under `not_warm`; that is normal. `global` rows must not appear after the flip.
- Compare the Trades tab and your warframe.market order list once: every live create/update in the log should match an order there.

## 4. Rollback

1. Press **Stop**. Turn **Dry-run** on. Press **Start** if you want simulation to continue.
2. Orders the trader created live stay on warframe.market. Either delete them by hand on the site, or set **Delete buy orders on stop** on, Start once live, and Stop: the stop sequence deletes the real buy orders (sell orders are not touched by that setting).
3. If the database looks wrong, restore last night's backup with the README's Restore steps.

## 5. Record it

Append the outcome to `docs/PHASE-5-ACCEPTANCE.md` under "The flip": date and time (UTC), the first live order id, the count of `Route: Live` items in the first cycle, whether the trader stopped in the first hour and why, and any manual order cleanup you did.
```

- [ ] **Step 3: Link it from the README**, after the **Restore** bullet:

```markdown
- **Going live:** follow `docs/GO-LIVE-RUNBOOK.md` once the dry-run review passes. Nothing turns dry-run off automatically.
```

- [ ] **Step 4: Check the two documents render** (`grep -c "^- \[ \]" docs/GO-LIVE-RUNBOOK.md` prints 9) and that every UI label you wrote exists in `en.json` (`grep -n "<label>" web/public/lang/en.json` for each).

- [ ] **Step 5: Commit and push**

```bash
git add docs/GO-LIVE-RUNBOOK.md README.md
git commit -m "docs: add the go-live runbook and link it from the README

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 6: Deploy the prep and record acceptance (needs the user)

**Files:**
- Create: `docs/PHASE-5-ACCEPTANCE.md`

**Interfaces:**
- Consumes: the branch tip after the final whole-branch review and fix wave; the ockohome deploy procedure from `docs/PHASE-4D-ACCEPTANCE.md` and the session notes (rsync excluding `.git target web/node_modules web/dist secrets .env .superpowers backups`, then `docker compose up -d --build`, `ssh -o ClearAllForwardings=yes`).
- Produces: the acceptance record with the G7 "now" checks; the "later" flip section left as a dated placeholder heading (the only permitted one, because the flip is a future event).

- [ ] **Step 1: Ask the user for the go-ahead to deploy.** Deploying to ockohome always needs an explicit yes. Show the commit to deploy and the rsync dry-run deletion list first.

- [ ] **Step 2: Deploy** (rsync the worktree, `docker compose up -d --build`, wait for healthy, capture the boot log). Confirm `Database ready`, `Housekeeping Started`, no panic, no `CRITICAL`, no `Trader started` at boot.

- [ ] **Step 3: Run the G7 "now" checks with the user in the browser** (the trader is in dry-run throughout; nothing goes live):

| # | Check | How |
|---|---|---|
| 1 | Dry-run Start logs `Simulated delete of order … (global) N/50` lines, no `Deleted order with ID` | Trader panel Start with `auto_delete` still on (it is in dry-run), read the Log tab |
| 2 | Idle: `Checking items...` repeats about every 30 s | Leave it running 2 minutes, count the lines on the Log tab |
| 3 | Stop takes effect within about a second during the idle pause | Press Stop mid-pause; the badge leaves Trading at once and `Trader stopped: Stop button` appears |
| 4 | Dry-run log tab shows the summary for 1, 7 and 30 days; counts match the paged rows | Switch the selector; compare the `delete` count with the `AutoDelete` rows in the table |
| 5 | Runbook exists and its labels match the UI | Open `docs/GO-LIVE-RUNBOOK.md` next to Settings and the Trader panel |

- [ ] **Step 4: Write `docs/PHASE-5-ACCEPTANCE.md`** in the same shape as `docs/PHASE-4D-ACCEPTANCE.md`: header with server, branch, commit, deploy time; local gate numbers; deploy evidence; the table above with results and log excerpts; rulings made during execution; follow-ups. End with:

```markdown
## The flip (to be appended on or after 2026-09-22)

Not done yet. Follow `docs/GO-LIVE-RUNBOOK.md` and record the outcome here.
```

- [ ] **Step 5: Commit and push**

```bash
git add docs/PHASE-5-ACCEPTANCE.md
git commit -m "docs: record phase 5 go-live prep acceptance

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

The merge into `main` is the controller's, after the gate is green on the branch tip.

---

## Self-review

- **Spec coverage.** G1 → Task 1. G2 → Task 2. G3 → Tasks 3–4. G4 → Task 5. G5 (not built) → no task, by design. G6 tests → Tasks 1–4 each carry theirs; `item` golden tests are adjusted in Task 2 Step 6. G7 "now" → Task 6; G7 "later" → runbook §5 and the acceptance record's dated section.
- **Placeholders.** The runbook's angle-bracket labels are filled in Task 5 from Step 1's greps before the file is written; the acceptance record's "flip" heading is a dated future section, not a TODO.
- **Type consistency.** `delete -> Result<Route, Error>` (Task 1) is matched on in Task 1 Step 4. `check -> Result<usize, Error>` (Task 2) matches `Fut: Future<Output = Result<usize, Error>>` in `run_loop`. `DryRunSummary { since, by_action, by_item }` field names in Task 3 match the TypeScript interfaces and the tab's `summary.by_action` / `summary.by_item` in Task 4. The RPC name `trader_dry_run_summary` is identical in `rpc.rs`, the rpc test, `dryRunSummary()` and the check script's expectation.
