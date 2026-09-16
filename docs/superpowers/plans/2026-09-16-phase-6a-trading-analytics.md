# Phase 6a: Trading Analytics — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task with Opus implementers and a reviewer between tasks. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Four new tabs on the Trading Analytics page — Items P&L, Stock performance, Trading partners, Profit timeline — each backed by one read-only RPC that aggregates the existing `transaction` and `stock_item` tables in SQL.

**Architecture:** A new `qf_core::analytics` module holds the SQL (`analytics/store.rs`) and a new `commands/analytics.rs` exposes four commands through the existing `rpc_table!`. Stock performance joins stock rows to market stats through the trader's own `StatsPriceSource`, so the sub-type mapping is the trader's. The web adds an `api/analytics` module, a shared date-range hook, and four tab components using `mantine-datatable` and `chart.js`, both already installed.

**Tech Stack:** Rust (sea-orm raw SQL over SQLite, serde), React 19 + Mantine 9 + mantine-datatable 8 + react-chartjs-2 + TanStack Query 5, pnpm 11.3.0.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md` — read **§22 (A1–A9)** first. §22 takes precedence over everything older.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-phase-6a`, branch `phase-6a-trading-analytics`, created from `main` after phase 5 is merged. Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target`.
- **Tests:** `cargo test -p utils --lib`, `cargo test -p qf_core --lib`, `cargo test -p qf-server`; never bare `--workspace`. Web: `python3 scripts/check-rpc-commands.py` and `(cd web && pnpm build)`. Output pristine.
- **Dates (A1):** RPC args `from: String`, `to: String`; a row is in range when `julianday(created_at) >= julianday(?) AND julianday(created_at) < julianday(?)` — `julianday()` on both sides because imported rows use `2026-05-18T01:17:52.787000659+00:00` and sea-orm writes `2026-09-16 08:10:37.123456 +00:00`, and plain text comparison would order those wrongly. The web sends `to` as the day after the picked end date. Default range: last 30 days, stored under `localStorage` key `trading_analytics_range`.
- **Exact names:** RPCs `analytics_items`, `analytics_stock`, `analytics_partners`, `analytics_timeline`; tab ids `items`, `stock`, `partners`, `timeline`; en.json keys under `pages.trading_analytics.tabs.{items,stock,partners,timeline}` (never `item`, which already exists); `bucket` is `"day"` or `"week"` and nothing else.
- **Profit** always sums the sale rows' existing `profit` column; never recompute it.
- **`en.json`** by targeted insertion only (Python: load, insert under the existing key, dump with `indent=2, ensure_ascii=False`, trailing newline); confirm it parses and the diff touches only the new keys.
- **Commits:** conventional commits ending with exactly `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`; `git push` after each task. Never touch `main`.
- Tasks 1–5 do not touch ockohome; Task 6 does and asks the user first.

## File Structure (end of phase 6a)

```
crates/qf_core/src/analytics/mod.rs                    NEW  pub mod store; row structs
crates/qf_core/src/analytics/store.rs                  NEW  items, partners, timeline, stock SQL + tests
crates/qf_core/src/lib.rs                              MOD  pub mod analytics;
crates/qf_core/src/commands/analytics.rs               NEW  four commands
crates/qf_core/src/commands/mod.rs                     MOD  pub mod analytics;
crates/qf_core/src/commands/rpc.rs                     MOD  four rows + test
web/src/api/analytics/index.ts                         NEW  AnalyticsModule
web/src/api/index.ts                                   MOD  register analytics
web/src/types/tauri.type.ts                            MOD  Analytics* types
web/src/pages/trading_analytics/range.ts               NEW  useAnalyticsRange hook
web/src/pages/trading_analytics/Tabs/Items/index.tsx   NEW
web/src/pages/trading_analytics/Tabs/Stock/index.tsx   NEW
web/src/pages/trading_analytics/Tabs/Partners/index.tsx NEW
web/src/pages/trading_analytics/Tabs/Timeline/index.tsx NEW
web/src/pages/trading_analytics/Tabs/index.ts          MOD  exports
web/src/pages/trading_analytics/index.tsx              MOD  four tabs
web/public/lang/en.json                                MOD  strings
docs/PHASE-6A-ACCEPTANCE.md                            NEW  (Task 6)
```

---

### Task 1: `analytics::store` — items, partners, timeline queries

**Files:**
- Create: `crates/qf_core/src/analytics/mod.rs`, `crates/qf_core/src/analytics/store.rs`
- Modify: `crates/qf_core/src/lib.rs` (add `pub mod analytics;` next to `pub mod collector;`)

**Interfaces:**
- Consumes: `crate::collector::{db_err, stmt}` and `crate::collector::store::exec` (pub(crate)); `crate::trader::store::tests::db()` (creates a migrated SQLite file database).
- Produces (in `analytics/mod.rs`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemRow { pub wfm_id: String, pub wfm_url: String, pub item_name: String, pub sub_type: String, pub purchases: i64, pub bought_qty: i64, pub spend: i64, pub sales: i64, pub sold_qty: i64, pub revenue: i64, pub profit: i64, pub avg_buy: Option<f64>, pub avg_sell: Option<f64>, pub avg_days_held: Option<f64> }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PartnerRow { pub user_name: String, pub trades: i64, pub bought_count: i64, pub bought_plat: i64, pub sold_count: i64, pub sold_plat: i64, pub profit: i64, pub last_trade_at: String }
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)] #[serde(rename_all = "lowercase")]
pub enum Bucket { Day, Week }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimelineRow { pub bucket_start: String, pub sales: i64, pub purchases: i64, pub revenue: i64, pub expenses: i64, pub profit: i64, pub cumulative_profit: i64 }
// analytics/store.rs
pub async fn items(conn: &DatabaseConnection, from: &str, to: &str) -> Result<Vec<ItemRow>, Error>;
pub async fn partners(conn: &DatabaseConnection, from: &str, to: &str) -> Result<Vec<PartnerRow>, Error>;
pub async fn timeline(conn: &DatabaseConnection, from: &str, to: &str, bucket: Bucket) -> Result<Vec<TimelineRow>, Error>;
```

- [ ] **Step 1: Create the module skeleton.** `analytics/mod.rs`:

```rust
//! Read-only aggregates over the transaction and stock tables (spec §22).

pub mod store;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemRow {
    pub wfm_id: String,
    pub wfm_url: String,
    pub item_name: String,
    pub sub_type: String,
    pub purchases: i64,
    pub bought_qty: i64,
    pub spend: i64,
    pub sales: i64,
    pub sold_qty: i64,
    pub revenue: i64,
    pub profit: i64,
    pub avg_buy: Option<f64>,
    pub avg_sell: Option<f64>,
    pub avg_days_held: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PartnerRow {
    pub user_name: String,
    pub trades: i64,
    pub bought_count: i64,
    pub bought_plat: i64,
    pub sold_count: i64,
    pub sold_plat: i64,
    pub profit: i64,
    pub last_trade_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Bucket {
    Day,
    Week,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimelineRow {
    pub bucket_start: String,
    pub sales: i64,
    pub purchases: i64,
    pub revenue: i64,
    pub expenses: i64,
    pub profit: i64,
    pub cumulative_profit: i64,
}
```

Add `pub mod analytics;` to `crates/qf_core/src/lib.rs`, and `analytics/store.rs` with only the `use` lines and an empty `tests` module for now:

```rust
//! SQL for the analytics tabs (spec §22 A2, A4, A5).

use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::Error;

use super::{Bucket, ItemRow, PartnerRow, TimelineRow};
use crate::collector::{db_err, stmt};

const RANGE: &str = "julianday(created_at) >= julianday(?) AND julianday(created_at) < julianday(?)";
```

- [ ] **Step 2: Write the failing tests** at the bottom of `analytics/store.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::store::exec;

    async fn db() -> (tempfile::TempDir, DatabaseConnection) {
        crate::trader::store::tests::db().await
    }

    /// Inserts one transaction row the way the desktop import and the sale handler store them.
    async fn tx(conn: &DatabaseConnection, kind: &str, url: &str, sub_type: Option<&str>, qty: i64, price: i64, profit: Option<i64>, user: &str, at: &str) {
        exec(
            conn,
            "Test:Tx",
            "INSERT INTO \"transaction\" (wfm_id, wfm_url, item_name, item_type, item_unique_name, sub_type, tags, transaction_type, quantity, user_name, price, profit, credits, created_at, updated_at)
             VALUES (?, ?, ?, 'item', 'N/A', ?, '', ?, ?, ?, ?, ?, 0, ?, ?)",
            vec![
                format!("id-{url}").into(), url.into(), url.replace('_', " ").into(), sub_type.into(), kind.into(),
                qty.into(), user.into(), price.into(), profit.into(), at.into(), at.into(),
            ],
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn items_aggregate_per_item_and_measure_days_held_from_the_latest_prior_purchase() {
        let (_dir, conn) = db().await;
        // Bought 2 at 50 each on the 1st (price is the row total), 1 at 36 on the 5th, sold 1 for 60 on the 8th (profit 24 written at sale time).
        tx(&conn, "purchase", "galvanized_shot", Some("{\"rank\":0}"), 2, 100, None, "alice", "2026-09-01T10:00:00.000000000+00:00").await;
        tx(&conn, "purchase", "galvanized_shot", Some("{\"rank\":0}"), 1, 36, None, "bob", "2026-09-05T10:00:00.000000000+00:00").await;
        tx(&conn, "sale", "galvanized_shot", Some("{\"rank\":0}"), 1, 60, Some(24), "carol", "2026-09-08T10:00:00.000000000+00:00").await;
        // A different variant of the same item is its own row.
        tx(&conn, "sale", "galvanized_shot", Some("{\"rank\":5}"), 1, 90, Some(40), "dave", "2026-09-08T11:00:00.000000000+00:00").await;
        // Outside the range (to is exclusive).
        tx(&conn, "sale", "galvanized_shot", Some("{\"rank\":0}"), 1, 999, Some(999), "eve", "2026-09-10 00:00:00.000000 +00:00").await;

        let rows = items(&conn, "2026-09-01", "2026-09-10").await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].sub_type, "{\"rank\":5}", "sorted by profit desc");
        let shot = &rows[1];
        assert_eq!((shot.purchases, shot.bought_qty, shot.spend), (2, 3, 136));
        assert_eq!((shot.sales, shot.sold_qty, shot.revenue, shot.profit), (1, 1, 60, 24));
        assert!((shot.avg_buy.unwrap() - 136.0 / 3.0).abs() < 1e-9);
        assert_eq!(shot.avg_sell, Some(60.0));
        assert!((shot.avg_days_held.unwrap() - 3.0).abs() < 1e-6, "sale on the 8th, latest prior purchase on the 5th");
        assert_eq!(rows[0].avg_days_held, None, "no purchase for rank 5");
    }

    #[tokio::test]
    async fn partners_split_directions_and_skip_empty_names() {
        let (_dir, conn) = db().await;
        tx(&conn, "purchase", "a", None, 1, 10, None, "alice", "2026-09-01T10:00:00.000000000+00:00").await;
        tx(&conn, "sale", "b", None, 2, 50, Some(20), "alice", "2026-09-03T10:00:00.000000000+00:00").await;
        tx(&conn, "sale", "c", None, 1, 5, Some(1), "", "2026-09-03T10:00:00.000000000+00:00").await;
        let rows = partners(&conn, "2026-09-01", "2026-09-10").await.unwrap();
        assert_eq!(
            rows,
            vec![PartnerRow { user_name: "alice".into(), trades: 2, bought_count: 1, bought_plat: 10, sold_count: 1, sold_plat: 50, profit: 20, last_trade_at: "2026-09-03T10:00:00.000000000+00:00".into() }]
        );
    }

    #[tokio::test]
    async fn timeline_buckets_by_day_or_iso_week_with_a_running_total() {
        let (_dir, conn) = db().await;
        // Wed 2026-09-02 and Sun 2026-09-06 are the same ISO week (Mon 08-31); Mon 2026-09-07 starts the next.
        tx(&conn, "sale", "a", None, 1, 30, Some(10), "u", "2026-09-02T10:00:00.000000000+00:00").await;
        tx(&conn, "purchase", "b", None, 1, 20, None, "u", "2026-09-06T10:00:00.000000000+00:00").await;
        tx(&conn, "sale", "c", None, 1, 40, Some(15), "u", "2026-09-07T10:00:00.000000000+00:00").await;

        let days = timeline(&conn, "2026-09-01", "2026-09-10", Bucket::Day).await.unwrap();
        assert_eq!(days.iter().map(|r| r.bucket_start.as_str()).collect::<Vec<_>>(), vec!["2026-09-02", "2026-09-06", "2026-09-07"]);
        assert_eq!(days.iter().map(|r| r.cumulative_profit).collect::<Vec<_>>(), vec![10, 10, 25]);
        assert_eq!((days[1].purchases, days[1].expenses, days[1].profit), (1, 20, 0));

        let weeks = timeline(&conn, "2026-09-01", "2026-09-10", Bucket::Week).await.unwrap();
        assert_eq!(
            weeks,
            vec![
                TimelineRow { bucket_start: "2026-08-31".into(), sales: 1, purchases: 1, revenue: 30, expenses: 20, profit: 10, cumulative_profit: 10 },
                TimelineRow { bucket_start: "2026-09-07".into(), sales: 1, purchases: 0, revenue: 40, expenses: 0, profit: 15, cumulative_profit: 25 },
            ]
        );
    }
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib analytics::`
Expected: compile errors, `items`/`partners`/`timeline` not found.

- [ ] **Step 4: Implement the three queries** in `analytics/store.rs`:

```rust
/// One row per item and variant with a transaction in range, profit descending (spec §22 A2).
pub async fn items(conn: &DatabaseConnection, from: &str, to: &str) -> Result<Vec<ItemRow>, Error> {
    const C: &str = "Analytics:Items";
    let sql = format!(
        "SELECT t.wfm_id, t.wfm_url, t.item_name, COALESCE(t.sub_type, '') AS sub_type,
                SUM(t.transaction_type = 'purchase') AS purchases,
                SUM(CASE WHEN t.transaction_type = 'purchase' THEN t.quantity ELSE 0 END) AS bought_qty,
                SUM(CASE WHEN t.transaction_type = 'purchase' THEN t.price ELSE 0 END) AS spend,
                SUM(t.transaction_type = 'sale') AS sales,
                SUM(CASE WHEN t.transaction_type = 'sale' THEN t.quantity ELSE 0 END) AS sold_qty,
                SUM(CASE WHEN t.transaction_type = 'sale' THEN t.price ELSE 0 END) AS revenue,
                SUM(CASE WHEN t.transaction_type = 'sale' THEN COALESCE(t.profit, 0) ELSE 0 END) AS profit,
                AVG(CASE WHEN t.transaction_type = 'sale' THEN
                    (SELECT julianday(t.created_at) - julianday(MAX(p.created_at)) FROM \"transaction\" p
                      WHERE p.wfm_url = t.wfm_url AND COALESCE(p.sub_type, '') = COALESCE(t.sub_type, '')
                        AND p.transaction_type = 'purchase' AND julianday(p.created_at) <= julianday(t.created_at))
                    END) AS avg_days_held
         FROM \"transaction\" t
         WHERE {RANGE}
         GROUP BY t.wfm_url, COALESCE(t.sub_type, '')
         ORDER BY profit DESC, t.item_name"
    );
    conn.query_all(stmt(&sql, vec![from.into(), to.into()]))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            let bought_qty: i64 = r.try_get("", "bought_qty").map_err(|e| db_err(C, e))?;
            let sold_qty: i64 = r.try_get("", "sold_qty").map_err(|e| db_err(C, e))?;
            let spend: i64 = r.try_get("", "spend").map_err(|e| db_err(C, e))?;
            let revenue: i64 = r.try_get("", "revenue").map_err(|e| db_err(C, e))?;
            Ok(ItemRow {
                wfm_id: r.try_get("", "wfm_id").map_err(|e| db_err(C, e))?,
                wfm_url: r.try_get("", "wfm_url").map_err(|e| db_err(C, e))?,
                item_name: r.try_get("", "item_name").map_err(|e| db_err(C, e))?,
                sub_type: r.try_get("", "sub_type").map_err(|e| db_err(C, e))?,
                purchases: r.try_get("", "purchases").map_err(|e| db_err(C, e))?,
                bought_qty,
                spend,
                sales: r.try_get("", "sales").map_err(|e| db_err(C, e))?,
                sold_qty,
                revenue,
                profit: r.try_get("", "profit").map_err(|e| db_err(C, e))?,
                avg_buy: (bought_qty > 0).then(|| spend as f64 / bought_qty as f64),
                avg_sell: (sold_qty > 0).then(|| revenue as f64 / sold_qty as f64),
                avg_days_held: r.try_get("", "avg_days_held").map_err(|e| db_err(C, e))?,
            })
        })
        .collect()
}

/// One row per non-empty trading partner, most trades first (spec §22 A4).
pub async fn partners(conn: &DatabaseConnection, from: &str, to: &str) -> Result<Vec<PartnerRow>, Error> {
    const C: &str = "Analytics:Partners";
    let sql = format!(
        "SELECT user_name, COUNT(*) AS trades,
                SUM(transaction_type = 'purchase') AS bought_count,
                SUM(CASE WHEN transaction_type = 'purchase' THEN price ELSE 0 END) AS bought_plat,
                SUM(transaction_type = 'sale') AS sold_count,
                SUM(CASE WHEN transaction_type = 'sale' THEN price ELSE 0 END) AS sold_plat,
                SUM(CASE WHEN transaction_type = 'sale' THEN COALESCE(profit, 0) ELSE 0 END) AS profit,
                MAX(created_at) AS last_trade_at
         FROM \"transaction\"
         WHERE user_name <> '' AND {RANGE}
         GROUP BY user_name
         ORDER BY trades DESC, user_name"
    );
    conn.query_all(stmt(&sql, vec![from.into(), to.into()]))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            Ok(PartnerRow {
                user_name: r.try_get("", "user_name").map_err(|e| db_err(C, e))?,
                trades: r.try_get("", "trades").map_err(|e| db_err(C, e))?,
                bought_count: r.try_get("", "bought_count").map_err(|e| db_err(C, e))?,
                bought_plat: r.try_get("", "bought_plat").map_err(|e| db_err(C, e))?,
                sold_count: r.try_get("", "sold_count").map_err(|e| db_err(C, e))?,
                sold_plat: r.try_get("", "sold_plat").map_err(|e| db_err(C, e))?,
                profit: r.try_get("", "profit").map_err(|e| db_err(C, e))?,
                last_trade_at: r.try_get("", "last_trade_at").map_err(|e| db_err(C, e))?,
            })
        })
        .collect()
}

/// Per-day or per-ISO-week totals with a running profit total (spec §22 A5).
pub async fn timeline(conn: &DatabaseConnection, from: &str, to: &str, bucket: Bucket) -> Result<Vec<TimelineRow>, Error> {
    const C: &str = "Analytics:Timeline";
    // 'weekday 1' after stepping back six days lands on the Monday of the row's ISO week.
    let start = match bucket {
        Bucket::Day => "date(created_at)",
        Bucket::Week => "date(created_at, '-6 days', 'weekday 1')",
    };
    let sql = format!(
        "SELECT {start} AS bucket_start,
                SUM(transaction_type = 'sale') AS sales,
                SUM(transaction_type = 'purchase') AS purchases,
                SUM(CASE WHEN transaction_type = 'sale' THEN price ELSE 0 END) AS revenue,
                SUM(CASE WHEN transaction_type = 'purchase' THEN price ELSE 0 END) AS expenses,
                SUM(CASE WHEN transaction_type = 'sale' THEN COALESCE(profit, 0) ELSE 0 END) AS profit
         FROM \"transaction\"
         WHERE {RANGE}
         GROUP BY bucket_start
         ORDER BY bucket_start"
    );
    let mut cumulative = 0;
    conn.query_all(stmt(&sql, vec![from.into(), to.into()]))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            let profit: i64 = r.try_get("", "profit").map_err(|e| db_err(C, e))?;
            cumulative += profit;
            Ok(TimelineRow {
                bucket_start: r.try_get("", "bucket_start").map_err(|e| db_err(C, e))?,
                sales: r.try_get("", "sales").map_err(|e| db_err(C, e))?,
                purchases: r.try_get("", "purchases").map_err(|e| db_err(C, e))?,
                revenue: r.try_get("", "revenue").map_err(|e| db_err(C, e))?,
                expenses: r.try_get("", "expenses").map_err(|e| db_err(C, e))?,
                profit,
                cumulative_profit: cumulative,
            })
        })
        .collect()
}
```

If `SUM(transaction_type = 'purchase')` comes back as a type sea-orm cannot read into `i64`, wrap it as `CAST(SUM(...) AS INTEGER)`; do the same for `julianday` results into `f64` if needed. Do not change the semantics.

- [ ] **Step 5: Run the tests**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib analytics::`
Expected: 3 passed.

- [ ] **Step 6: Commit and push**

```bash
git add crates/qf_core/src/lib.rs crates/qf_core/src/analytics
git commit -m "feat(analytics): add item, partner and timeline aggregates over transactions

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push -u origin phase-6a-trading-analytics
```

---

### Task 2: `analytics::store::stock` — stock rows joined to market stats

**Files:**
- Modify: `crates/qf_core/src/analytics/mod.rs` (add `StockRow`), `crates/qf_core/src/analytics/store.rs` (add `stock`, test)

**Interfaces:**
- Consumes: `crate::trader::price_source::{PriceSource, StatsPriceSource}` (`find_by(&self, wfm_id: &str, sub_type: &Option<utils::SubType>) -> Option<ItemPriceInfo>` with fields `median: f64`, `moving_avg: Option<f64>`, `volume: f64`, `warm: bool`); `chrono`.
- Produces:

```rust
pub struct StockRow { pub id: i64, pub wfm_id: String, pub wfm_url: String, pub item_name: String, pub sub_type: String, pub owned: i64, pub bought: i64, pub list_price: Option<i64>, pub status: String, pub created_at: String, pub days_in_stock: f64, pub median: Option<f64>, pub moving_avg: Option<f64>, pub volume: Option<f64>, pub warm: bool, pub unrealised: Option<f64>, pub list_vs_median: Option<f64> }
pub async fn stock(conn: &DatabaseConnection, prices: &dyn PriceSource, now: DateTime<Utc>) -> Result<Vec<StockRow>, Error>;
```

- [ ] **Step 1: Add the row struct** to `analytics/mod.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StockRow {
    pub id: i64,
    pub wfm_id: String,
    pub wfm_url: String,
    pub item_name: String,
    pub sub_type: String,
    pub owned: i64,
    pub bought: i64,
    pub list_price: Option<i64>,
    pub status: String,
    pub created_at: String,
    pub days_in_stock: f64,
    pub median: Option<f64>,
    pub moving_avg: Option<f64>,
    pub volume: Option<f64>,
    pub warm: bool,
    pub unrealised: Option<f64>,
    pub list_vs_median: Option<f64>,
}
```

- [ ] **Step 2: Write the failing test** in `analytics/store.rs` `tests`:

```rust
    #[tokio::test]
    async fn stock_joins_market_stats_through_the_trader_price_source_and_sorts_by_unrealised() {
        use crate::collector::stats::ItemStats;
        use crate::trader::price_source::StatsPriceSource;
        let (_dir, conn) = db().await;
        for (name, sub_type, owned, bought, list) in [("a", Some("{\"rank\":0}"), 2, 40, Some(55)), ("b", None, 1, 100, None), ("gone", None, 0, 1, None)] {
            exec(
                &conn,
                "Test:Stock",
                "INSERT INTO stock_item (wfm_id, wfm_url, item_name, item_unique_name, sub_type, bought, list_price, owned, is_hidden, created_at, updated_at, status, price_history, properties)
                 VALUES (?, ?, ?, 'N/A', ?, ?, ?, ?, 0, '2026-09-01T00:00:00.000000000+00:00', '2026-09-01T00:00:00.000000000+00:00', 'pending', '[]', '{}')",
                vec![format!("id-{name}").into(), name.into(), name.into(), sub_type.into(), bought.into(), list.into(), owned.into()],
            )
            .await
            .unwrap();
        }
        let stats = vec![ItemStats {
            item_id: "id-a".into(), sub_type: "rank=0".into(), volume: 2.5, avg_price: Some(50.0), moving_avg: Some(48.0), profit: Some(5.0),
            min_price: Some(40), max_price: Some(60), median: Some(50.0), history_days: 8, warm: true, updated_at: "2026-09-10T00:00:00Z".into(),
        }];
        let prices = StatsPriceSource::from_stats(stats, |id| Some(id.trim_start_matches("id-").to_string()));
        let now = crate::collector::parse_ts("2026-09-11T00:00:00Z").unwrap();

        let rows = stock(&conn, &prices, now).await.unwrap();
        assert_eq!(rows.iter().map(|r| r.wfm_url.as_str()).collect::<Vec<_>>(), vec!["a", "b"], "owned = 0 rows are skipped; known stats first");
        let a = &rows[0];
        assert_eq!((a.owned, a.bought, a.list_price, a.warm), (2, 40, Some(55), true));
        assert_eq!((a.median, a.moving_avg, a.volume), (Some(50.0), Some(48.0), Some(2.5)));
        assert_eq!(a.unrealised, Some(20.0), "(50 - 40) * 2");
        assert_eq!(a.list_vs_median, Some(5.0));
        assert!((a.days_in_stock - 10.0).abs() < 1e-6);
        let b = &rows[1];
        assert_eq!((b.median, b.unrealised, b.list_vs_median, b.warm), (None, None, None, false));
    }
```

- [ ] **Step 3: Run it to verify it fails**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib analytics::store::tests::stock_joins`
Expected: compile error, `stock` not found.

- [ ] **Step 4: Implement** in `analytics/store.rs` (add `use chrono::{DateTime, Utc};`, `use crate::trader::price_source::PriceSource;`, `use super::StockRow;`):

```rust
/// Every owned stock row with the trader's view of its market (spec §22 A3), unrealised profit descending, unknown market last.
pub async fn stock(conn: &DatabaseConnection, prices: &dyn PriceSource, now: DateTime<Utc>) -> Result<Vec<StockRow>, Error> {
    const C: &str = "Analytics:Stock";
    let mut rows = conn
        .query_all(stmt(
            "SELECT id, wfm_id, wfm_url, item_name, COALESCE(sub_type, '') AS sub_type, owned, bought, list_price, status, created_at,
                    julianday(?) - julianday(created_at) AS days_in_stock
             FROM stock_item WHERE owned > 0",
            vec![crate::collector::ts(now).into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            let sub_type: String = r.try_get("", "sub_type").map_err(|e| db_err(C, e))?;
            let wfm_id: String = r.try_get("", "wfm_id").map_err(|e| db_err(C, e))?;
            let owned: i64 = r.try_get("", "owned").map_err(|e| db_err(C, e))?;
            let bought: i64 = r.try_get("", "bought").map_err(|e| db_err(C, e))?;
            let list_price: Option<i64> = r.try_get("", "list_price").map_err(|e| db_err(C, e))?;
            // The stored sub_type is the entity's JSON; the price source keys on the same entity type the trader uses.
            let parsed: Option<utils::SubType> = if sub_type.is_empty() { None } else { serde_json::from_str(&sub_type).ok() };
            let info = prices.find_by(&wfm_id, &parsed);
            let median = info.as_ref().map(|i| i.median);
            Ok(StockRow {
                id: r.try_get("", "id").map_err(|e| db_err(C, e))?,
                wfm_id,
                wfm_url: r.try_get("", "wfm_url").map_err(|e| db_err(C, e))?,
                item_name: r.try_get("", "item_name").map_err(|e| db_err(C, e))?,
                sub_type,
                owned,
                bought,
                list_price,
                status: r.try_get("", "status").map_err(|e| db_err(C, e))?,
                created_at: r.try_get("", "created_at").map_err(|e| db_err(C, e))?,
                days_in_stock: r.try_get("", "days_in_stock").map_err(|e| db_err(C, e))?,
                median,
                moving_avg: info.as_ref().and_then(|i| i.moving_avg),
                volume: info.as_ref().map(|i| i.volume),
                warm: info.as_ref().is_some_and(|i| i.warm),
                unrealised: median.map(|m| (m - bought as f64) * owned as f64),
                list_vs_median: median.and_then(|m| list_price.map(|l| l as f64 - m)),
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    rows.sort_by(|a, b| match (a.unrealised, b.unrealised) {
        (Some(x), Some(y)) => y.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.item_name.cmp(&b.item_name),
    });
    Ok(rows)
}
```

`utils::SubType` derives `Deserialize` with `rank`/`variant`/`charges` fields (see `crates/utils/src/sub_type.rs`), so `serde_json::from_str::<utils::SubType>("{\"rank\":0}")` works. If the entity's `sub_type` column is stored with a different JSON shape on the server (check one row of `stock_item` in the test database after the desktop import script's format: it is `{"rank":N}`), keep `serde_json` and adjust nothing else.

- [ ] **Step 5: Run the analytics tests**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib analytics::`
Expected: 4 passed.

- [ ] **Step 6: Commit and push**

```bash
git add crates/qf_core/src/analytics
git commit -m "feat(analytics): add stock performance rows joined to the trader's price source

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 3: The four RPC commands

**Files:**
- Create: `crates/qf_core/src/commands/analytics.rs`
- Modify: `crates/qf_core/src/commands/mod.rs` (add `pub mod analytics;`), `crates/qf_core/src/commands/rpc.rs` (four rows after `market_item_history`, and a test)

**Interfaces:**
- Consumes: Tasks 1–2 functions; `crate::DATABASE`, `crate::utils::modules::states::cache_client()`, `StatsPriceSource::load(conn, &cache)`.
- Produces: RPCs `analytics_items { from, to }`, `analytics_stock {}`, `analytics_partners { from, to }`, `analytics_timeline { from, to, bucket }`.

- [ ] **Step 1: Write the failing rpc test** in `rpc.rs` `tests`, after `collector_commands_are_routable_and_validate_args`:

```rust
    #[tokio::test]
    async fn analytics_commands_are_routable_and_validate_args() {
        for name in ["analytics_items", "analytics_stock", "analytics_partners", "analytics_timeline"] {
            assert!(COMMANDS.contains(&name), "{name}");
        }
        assert!(dispatch("analytics_items", json!({"from": "2026-09-01"})).await.unwrap().is_err(), "to is required");
        assert!(dispatch("analytics_partners", json!({})).await.unwrap().is_err(), "from is required");
        assert!(
            dispatch("analytics_timeline", json!({"from": "2026-09-01", "to": "2026-09-10", "bucket": "month"})).await.unwrap().is_err(),
            "bucket must be day or week"
        );
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib commands::rpc::tests::analytics`
Expected: FAIL, `analytics_items` not in `COMMANDS`.

- [ ] **Step 3: Write the command module** `commands/analytics.rs`:

```rust
//! Trading Analytics RPCs (spec §22).

use chrono::Utc;
use utils::{get_location, Error};

use crate::analytics::{store, Bucket, ItemRow, PartnerRow, StockRow, TimelineRow};
use crate::trader::price_source::StatsPriceSource;
use crate::utils::modules::states;
use crate::DATABASE;

fn database() -> Result<&'static service::sea_orm::DatabaseConnection, Error> {
    DATABASE.get().ok_or_else(|| Error::new("Analytics:Rpc", "Database is not ready", get_location!()))
}

pub async fn analytics_items(from: String, to: String) -> Result<Vec<ItemRow>, Error> {
    store::items(database()?, &from, &to).await
}

pub async fn analytics_stock() -> Result<Vec<StockRow>, Error> {
    let conn = database()?;
    let prices = StatsPriceSource::load(conn, &states::cache_client()?).await?;
    store::stock(conn, &prices, Utc::now()).await
}

pub async fn analytics_partners(from: String, to: String) -> Result<Vec<PartnerRow>, Error> {
    store::partners(database()?, &from, &to).await
}

pub async fn analytics_timeline(from: String, to: String, bucket: Bucket) -> Result<Vec<TimelineRow>, Error> {
    store::timeline(database()?, &from, &to, bucket).await
}
```

Add `pub mod analytics;` to `commands/mod.rs` (alphabetically, before `app`).

- [ ] **Step 4: Register the rows** in `rpc.rs` after the `market_item_history` row (add `use crate::analytics::Bucket;` at the top):

```rust
    analytics_items => analytics::analytics_items { from: String, to: String },
    analytics_stock => analytics::analytics_stock {},
    analytics_partners => analytics::analytics_partners { from: String, to: String },
    analytics_timeline => analytics::analytics_timeline { from: String, to: String, bucket: Bucket },
```

- [ ] **Step 5: Run the suite and the RPC script**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib && python3 scripts/check-rpc-commands.py`
Expected: all pass; the script prints `0 missing` (it counts web calls with no server command) but its "used by web" count is 4 below the server count until Tasks 4–5 land. Do not add web code here.

- [ ] **Step 6: Commit and push**

```bash
git add crates/qf_core/src/commands/analytics.rs crates/qf_core/src/commands/mod.rs crates/qf_core/src/commands/rpc.rs
git commit -m "feat(rpc): expose analytics_items, analytics_stock, analytics_partners and analytics_timeline

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 4: Web — API module, types, range hook, Items and Stock tabs

**Files:**
- Create: `web/src/api/analytics/index.ts`, `web/src/pages/trading_analytics/range.ts`, `web/src/pages/trading_analytics/Tabs/Items/index.tsx`, `web/src/pages/trading_analytics/Tabs/Stock/index.tsx`
- Modify: `web/src/api/index.ts`, `web/src/types/tauri.type.ts`, `web/src/pages/trading_analytics/Tabs/index.ts`, `web/src/pages/trading_analytics/index.tsx`, `web/public/lang/en.json`

**Interfaces:**
- Consumes: the four RPCs from Task 3 with the row shapes from Tasks 1–2.
- Produces: `api.analytics.{items(from,to), stock(), partners(from,to), timeline(from,to,bucket)}` returning promises; `useAnalyticsRange()` returning `{ range, setRange, from, to }`; tabs `items` and `stock`.

- [ ] **Step 1: Types** in `tauri.type.ts`, after `DryRunSummary` (from phase 5):

```ts
  export interface AnalyticsItemRow {
    wfm_id: string;
    wfm_url: string;
    item_name: string;
    sub_type: string;
    purchases: number;
    bought_qty: number;
    spend: number;
    sales: number;
    sold_qty: number;
    revenue: number;
    profit: number;
    avg_buy?: number | null;
    avg_sell?: number | null;
    avg_days_held?: number | null;
  }
  export interface AnalyticsStockRow {
    id: number;
    wfm_id: string;
    wfm_url: string;
    item_name: string;
    sub_type: string;
    owned: number;
    bought: number;
    list_price?: number | null;
    status: string;
    created_at: string;
    days_in_stock: number;
    median?: number | null;
    moving_avg?: number | null;
    volume?: number | null;
    warm: boolean;
    unrealised?: number | null;
    list_vs_median?: number | null;
  }
  export interface AnalyticsPartnerRow {
    user_name: string;
    trades: number;
    bought_count: number;
    bought_plat: number;
    sold_count: number;
    sold_plat: number;
    profit: number;
    last_trade_at: string;
  }
  export type AnalyticsBucket = "day" | "week";
  export interface AnalyticsTimelineRow {
    bucket_start: string;
    sales: number;
    purchases: number;
    revenue: number;
    expenses: number;
    profit: number;
    cumulative_profit: number;
  }
```

- [ ] **Step 2: API module** `web/src/api/analytics/index.ts`:

```ts
import { TauriClient } from "..";
import { TauriTypes } from "$types";

export class AnalyticsModule {
  constructor(private readonly client: TauriClient) {}

  items(from: string, to: string) {
    return this.client.sendInvoke<TauriTypes.AnalyticsItemRow[]>("analytics_items", { from, to });
  }
  stock() {
    return this.client.sendInvoke<TauriTypes.AnalyticsStockRow[]>("analytics_stock");
  }
  partners(from: string, to: string) {
    return this.client.sendInvoke<TauriTypes.AnalyticsPartnerRow[]>("analytics_partners", { from, to });
  }
  timeline(from: string, to: string, bucket: TauriTypes.AnalyticsBucket) {
    return this.client.sendInvoke<TauriTypes.AnalyticsTimelineRow[]>("analytics_timeline", { from, to, bucket });
  }
}
```

Register it in `web/src/api/index.ts`: `import { AnalyticsModule } from "./analytics";`, a `analytics: AnalyticsModule;` field next to the others, and `this.analytics = new AnalyticsModule(this);` in the constructor.

- [ ] **Step 3: Range hook** `web/src/pages/trading_analytics/range.ts`:

```ts
import { useLocalStorage } from "@mantine/hooks";
import dayjs from "dayjs";

type Range = [string | null, string | null];

/** Shared inclusive date range for the analytics tabs; `to` is sent as the day after the picked end (spec §22 A1). */
export function useAnalyticsRange() {
  const [range, setRange] = useLocalStorage<Range>({
    key: "trading_analytics_range",
    defaultValue: [dayjs().subtract(30, "day").format("YYYY-MM-DD"), dayjs().format("YYYY-MM-DD")],
  });
  const from = range[0] ?? dayjs().subtract(30, "day").format("YYYY-MM-DD");
  const to = dayjs(range[1] ?? dayjs().format("YYYY-MM-DD")).add(1, "day").format("YYYY-MM-DD");
  return { range, setRange, from, to };
}
```

- [ ] **Step 4: Strings.** Insert with a script:

```bash
cd web && python3 - <<'EOF'
import json
p = "public/lang/en.json"
d = json.load(open(p, encoding="utf-8"))
tabs = d["pages"]["trading_analytics"]["tabs"]
for key in ("items", "stock", "partners", "timeline"):
    assert key not in tabs
tabs["items"] = {
  "title": "Items P&L",
  "range": "Date range",
  "search": "Search items",
  "columns": {"item": "Item", "sub_type": "Rank / variant", "purchases": "Bought", "bought_qty": "Qty", "spend": "Spend", "sales": "Sold", "sold_qty": "Qty", "revenue": "Revenue", "profit": "Profit", "avg_buy": "Avg buy", "avg_sell": "Avg sell", "avg_days_held": "Days held"},
  "empty": "No transactions in this range."
}
tabs["stock"] = {
  "title": "Stock performance",
  "columns": {"item": "Item", "sub_type": "Rank / variant", "owned": "Owned", "bought": "Bought at", "list_price": "Listed at", "median": "Median (7 d)", "moving_avg": "Moving avg", "volume": "Trades / day", "unrealised": "Unrealised", "list_vs_median": "List vs median", "days_in_stock": "Days in stock", "status": "Status"},
  "warm": "Warm",
  "cold": "Warming up",
  "no_stats": "Not tracked",
  "empty": "No stock."
}
tabs["partners"] = {
  "title": "Trading partners",
  "range": "Date range",
  "columns": {"user": "User", "trades": "Trades", "bought_count": "Bought from", "bought_plat": "Paid", "sold_count": "Sold to", "sold_plat": "Received", "profit": "Profit", "last_trade_at": "Last trade"},
  "empty": "No trading partners in this range."
}
tabs["timeline"] = {
  "title": "Profit timeline",
  "range": "Date range",
  "bucket": {"day": "Day", "week": "Week"},
  "profit": "Profit",
  "cumulative": "Cumulative profit",
  "totals": {"revenue": "Revenue", "expenses": "Expenses", "profit": "Profit", "sales": "Sales", "purchases": "Purchases"},
  "empty": "No transactions in this range."
}
with open(p, "w", encoding="utf-8") as f:
    json.dump(d, f, indent=2, ensure_ascii=False)
    f.write("\n")
EOF
python3 -c "import json; json.load(open('public/lang/en.json', encoding='utf-8'))" && git diff --stat public/lang/en.json
```

Expected: one contiguous insertion under `trading_analytics.tabs`. If re-serialisation touched other lines, revert and insert the block by text edit instead.

- [ ] **Step 5: Items tab** `Tabs/Items/index.tsx`:

```tsx
import api from "@api/index";
import { SearchField } from "@components/Forms/SearchField";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Group, Text } from "@mantine/core";
import { DatePickerInput } from "@mantine/dates";
import { useQuery } from "@tanstack/react-query";
import { DataTable, DataTableSortStatus } from "mantine-datatable";
import { useMemo, useState } from "react";
import { TauriTypes } from "$types";
import { useAnalyticsRange } from "../../range";

const num = (value?: number | null, digits = 1) => (value == null ? "—" : value.toFixed(digits));

/** Client-side sort on one column; strings compare with localeCompare, numbers numerically, nulls last. */
export function sortRows<T>(rows: T[], status: DataTableSortStatus<T>): T[] {
  const key = status.columnAccessor as keyof T;
  const dir = status.direction === "asc" ? 1 : -1;
  return [...rows].sort((a, b) => {
    const x = a[key] as unknown, y = b[key] as unknown;
    if (x == null && y == null) return 0;
    if (x == null) return 1;
    if (y == null) return -1;
    if (typeof x === "number" && typeof y === "number") return (x - y) * dir;
    return String(x).localeCompare(String(y)) * dir;
  });
}

export function ItemsPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`trading_analytics.tabs.items.${key}`, context);
  const { range, setRange, from, to } = useAnalyticsRange();
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<DataTableSortStatus<TauriTypes.AnalyticsItemRow>>({ columnAccessor: "profit", direction: "desc" });
  const { data, isFetching } = useQuery({
    queryKey: ["analytics_items", from, to],
    queryFn: () => api.analytics.items(from, to),
    enabled: !!isActive,
  });
  const rows = useMemo(() => {
    const filtered = (data ?? []).filter((r) => r.item_name.toLowerCase().includes(search.toLowerCase()));
    return sortRows(filtered, sort);
  }, [data, search, sort]);

  return (
    <>
      <Group mt="md" align="end">
        <DatePickerInput type="range" clearable label={t("range")} w={260} value={range as any} onChange={(v) => setRange(v as any)} />
        <SearchField value={search} onChange={setSearch} />
      </Group>
      <DataTable
        mt="md"
        striped
        fetching={isFetching}
        records={rows}
        idAccessor={(r) => `${r.wfm_url}|${r.sub_type}`}
        sortStatus={sort}
        onSortStatusChange={setSort}
        noRecordsText={t("empty")}
        columns={[
          { accessor: "item_name", title: t("columns.item"), sortable: true },
          { accessor: "sub_type", title: t("columns.sub_type"), render: (r) => r.sub_type || "—" },
          { accessor: "purchases", title: t("columns.purchases"), sortable: true },
          { accessor: "bought_qty", title: t("columns.bought_qty"), sortable: true },
          { accessor: "spend", title: t("columns.spend"), sortable: true },
          { accessor: "sales", title: t("columns.sales"), sortable: true },
          { accessor: "sold_qty", title: t("columns.sold_qty"), sortable: true },
          { accessor: "revenue", title: t("columns.revenue"), sortable: true },
          { accessor: "profit", title: t("columns.profit"), sortable: true, render: (r) => <Text c={r.profit < 0 ? "red" : "green"}>{r.profit}</Text> },
          { accessor: "avg_buy", title: t("columns.avg_buy"), sortable: true, render: (r) => num(r.avg_buy) },
          { accessor: "avg_sell", title: t("columns.avg_sell"), sortable: true, render: (r) => num(r.avg_sell) },
          { accessor: "avg_days_held", title: t("columns.avg_days_held"), sortable: true, render: (r) => num(r.avg_days_held) },
        ]}
      />
    </>
  );
}
```

Check `SearchField`'s actual props in `web/src/components/Forms/SearchField` and match them (the Transaction tab uses it; copy its usage). If `DatePickerInput`'s `value` type in the installed Mantine version is `[Date | null, Date | null]` rather than strings, convert: `value={range.map((d) => (d ? dayjs(d).toDate() : null))}` and `onChange={(v) => setRange(v.map((d) => (d ? dayjs(d).format("YYYY-MM-DD") : null)))}` — pick the one that type-checks and keep the stored form as `YYYY-MM-DD` strings.

- [ ] **Step 6: Stock tab** `Tabs/Stock/index.tsx`:

```tsx
import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Badge, Text } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { DataTable, DataTableSortStatus } from "mantine-datatable";
import { useMemo, useState } from "react";
import { TauriTypes } from "$types";
import { sortRows } from "../Items";

const num = (value?: number | null, digits = 1) => (value == null ? "—" : value.toFixed(digits));

export function StockPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`trading_analytics.tabs.stock.${key}`, context);
  const [sort, setSort] = useState<DataTableSortStatus<TauriTypes.AnalyticsStockRow>>({ columnAccessor: "unrealised", direction: "desc" });
  const { data, isFetching } = useQuery({ queryKey: ["analytics_stock"], queryFn: () => api.analytics.stock(), enabled: !!isActive, refetchInterval: 60_000 });
  const rows = useMemo(() => sortRows(data ?? [], sort), [data, sort]);
  const signed = (v?: number | null) => (v == null ? <Text c="dimmed">—</Text> : <Text c={v < 0 ? "red" : "green"}>{v.toFixed(0)}</Text>);

  return (
    <DataTable
      mt="md"
      striped
      fetching={isFetching}
      records={rows}
      idAccessor="id"
      sortStatus={sort}
      onSortStatusChange={setSort}
      noRecordsText={t("empty")}
      columns={[
        { accessor: "item_name", title: t("columns.item"), sortable: true },
        { accessor: "sub_type", title: t("columns.sub_type"), render: (r) => r.sub_type || "—" },
        { accessor: "owned", title: t("columns.owned"), sortable: true },
        { accessor: "bought", title: t("columns.bought"), sortable: true },
        { accessor: "list_price", title: t("columns.list_price"), sortable: true, render: (r) => r.list_price ?? "—" },
        { accessor: "median", title: t("columns.median"), sortable: true, render: (r) => num(r.median) },
        { accessor: "moving_avg", title: t("columns.moving_avg"), sortable: true, render: (r) => num(r.moving_avg) },
        { accessor: "volume", title: t("columns.volume"), sortable: true, render: (r) => num(r.volume, 2) },
        { accessor: "unrealised", title: t("columns.unrealised"), sortable: true, render: (r) => signed(r.unrealised) },
        { accessor: "list_vs_median", title: t("columns.list_vs_median"), sortable: true, render: (r) => signed(r.list_vs_median) },
        { accessor: "days_in_stock", title: t("columns.days_in_stock"), sortable: true, render: (r) => r.days_in_stock.toFixed(0) },
        {
          accessor: "warm",
          title: t("columns.status"),
          render: (r) => (r.median == null ? <Badge color="gray">{t("no_stats")}</Badge> : <Badge color={r.warm ? "green" : "yellow"}>{r.warm ? t("warm") : t("cold")}</Badge>),
        },
      ]}
    />
  );
}
```

- [ ] **Step 7: Wire the tabs.** `Tabs/index.ts` adds `export * from "./Items";` and `export * from "./Stock";`. In `pages/trading_analytics/index.tsx` extend the `tabs` array after the transaction entry:

```tsx
      { label: useTranslateTabs("items.title"), component: (isActive: boolean) => <ItemsPanel isActive={isActive} />, id: "items" },
      { label: useTranslateTabs("stock.title"), component: (isActive: boolean) => <StockPanel isActive={isActive} />, id: "stock" },
```

and import `ItemsPanel, StockPanel` from `./Tabs`.

- [ ] **Step 8: Build**

Run: `python3 scripts/check-rpc-commands.py && (cd web && pnpm build)`
Expected: the script's "used by web" count is still 2 below the server count (partners and timeline arrive in Task 5); tsc and vite clean.

- [ ] **Step 9: Commit and push**

```bash
git add web/src/api/analytics/index.ts web/src/api/index.ts web/src/types/tauri.type.ts web/src/pages/trading_analytics web/public/lang/en.json
git commit -m "feat(web): add Items P&L and Stock performance tabs to Trading Analytics

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 5: Web — Partners and Timeline tabs

**Files:**
- Create: `web/src/pages/trading_analytics/Tabs/Partners/index.tsx`, `web/src/pages/trading_analytics/Tabs/Timeline/index.tsx`
- Modify: `web/src/pages/trading_analytics/Tabs/index.ts`, `web/src/pages/trading_analytics/index.tsx`

**Interfaces:**
- Consumes: `api.analytics.partners/timeline`, `useAnalyticsRange`, `sortRows` from Task 4; strings already inserted in Task 4; `web/src/utils/chartjs.ts` already registers the chart.js scales and elements the app uses.

- [ ] **Step 1: Partners tab** `Tabs/Partners/index.tsx`:

```tsx
import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Group, Text } from "@mantine/core";
import { DatePickerInput } from "@mantine/dates";
import { useQuery } from "@tanstack/react-query";
import dayjs from "dayjs";
import { DataTable, DataTableSortStatus } from "mantine-datatable";
import { useMemo, useState } from "react";
import { TauriTypes } from "$types";
import { sortRows } from "../Items";
import { useAnalyticsRange } from "../../range";

export function PartnersPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`trading_analytics.tabs.partners.${key}`, context);
  const { range, setRange, from, to } = useAnalyticsRange();
  const [sort, setSort] = useState<DataTableSortStatus<TauriTypes.AnalyticsPartnerRow>>({ columnAccessor: "trades", direction: "desc" });
  const { data, isFetching } = useQuery({ queryKey: ["analytics_partners", from, to], queryFn: () => api.analytics.partners(from, to), enabled: !!isActive });
  const rows = useMemo(() => sortRows(data ?? [], sort), [data, sort]);

  return (
    <>
      <Group mt="md" align="end">
        <DatePickerInput type="range" clearable label={t("range")} w={260} value={range as any} onChange={(v) => setRange(v as any)} />
      </Group>
      <DataTable
        mt="md"
        striped
        fetching={isFetching}
        records={rows}
        idAccessor="user_name"
        sortStatus={sort}
        onSortStatusChange={setSort}
        noRecordsText={t("empty")}
        columns={[
          { accessor: "user_name", title: t("columns.user"), sortable: true },
          { accessor: "trades", title: t("columns.trades"), sortable: true },
          { accessor: "bought_count", title: t("columns.bought_count"), sortable: true },
          { accessor: "bought_plat", title: t("columns.bought_plat"), sortable: true },
          { accessor: "sold_count", title: t("columns.sold_count"), sortable: true },
          { accessor: "sold_plat", title: t("columns.sold_plat"), sortable: true },
          { accessor: "profit", title: t("columns.profit"), sortable: true, render: (r) => <Text c={r.profit < 0 ? "red" : "green"}>{r.profit}</Text> },
          { accessor: "last_trade_at", title: t("columns.last_trade_at"), sortable: true, render: (r) => dayjs(r.last_trade_at).format("YYYY-MM-DD HH:mm") },
        ]}
      />
    </>
  );
}
```

Use the same `DatePickerInput` value conversion Task 4 settled on.

- [ ] **Step 2: Timeline tab** `Tabs/Timeline/index.tsx`:

```tsx
import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Box, Group, Paper, SegmentedControl, SimpleGrid, Text, useMantineTheme } from "@mantine/core";
import { DatePickerInput } from "@mantine/dates";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Chart } from "react-chartjs-2";
import { TauriTypes } from "$types";
import { useAnalyticsRange } from "../../range";

export function TimelinePanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`trading_analytics.tabs.timeline.${key}`, context);
  const theme = useMantineTheme();
  const { range, setRange, from, to } = useAnalyticsRange();
  const [bucket, setBucket] = useState<TauriTypes.AnalyticsBucket>("day");
  const { data } = useQuery({ queryKey: ["analytics_timeline", from, to, bucket], queryFn: () => api.analytics.timeline(from, to, bucket), enabled: !!isActive });
  const rows = data ?? [];
  const sum = (key: keyof TauriTypes.AnalyticsTimelineRow) => rows.reduce((acc, r) => acc + (r[key] as number), 0);

  return (
    <>
      <Group mt="md" align="end">
        <DatePickerInput type="range" clearable label={t("range")} w={260} value={range as any} onChange={(v) => setRange(v as any)} />
        <SegmentedControl value={bucket} onChange={(v) => setBucket(v as TauriTypes.AnalyticsBucket)} data={[{ value: "day", label: t("bucket.day") }, { value: "week", label: t("bucket.week") }]} />
      </Group>
      <SimpleGrid cols={{ base: 2, md: 5 }} mt="md">
        {(["revenue", "expenses", "profit", "sales", "purchases"] as const).map((key) => (
          <Paper withBorder p="sm" key={key}>
            <Text size="xs" c="dimmed">
              {t(`totals.${key}`)}
            </Text>
            <Text fw={700}>{sum(key)}</Text>
          </Paper>
        ))}
      </SimpleGrid>
      <Paper withBorder p="sm" mt="md">
        {rows.length === 0 ? (
          <Text c="dimmed">{t("empty")}</Text>
        ) : (
          <Box h={360}>
            <Chart
              type="bar"
              options={{
                responsive: true,
                maintainAspectRatio: false,
                scales: { y: { position: "left" }, y1: { position: "right", grid: { drawOnChartArea: false } } },
              }}
              data={{
                labels: rows.map((r) => r.bucket_start),
                datasets: [
                  { type: "bar", label: t("profit"), data: rows.map((r) => r.profit), backgroundColor: theme.colors.green[6], yAxisID: "y" },
                  { type: "line", label: t("cumulative"), data: rows.map((r) => r.cumulative_profit), borderColor: theme.colors.blue[6], backgroundColor: theme.colors.blue[6], yAxisID: "y1" },
                ],
              }}
            />
          </Box>
        )}
      </Paper>
    </>
  );
}
```

If `Chart` from `react-chartjs-2` complains that the line dataset's controller is not registered, add `LineController, BarController` to the `ChartJS.register(...)` call in `web/src/utils/chartjs.ts` (import them from `chart.js`) — that file is the app's single registration point.

- [ ] **Step 3: Wire the tabs.** `Tabs/index.ts` adds `export * from "./Partners";` and `export * from "./Timeline";`. In `pages/trading_analytics/index.tsx` add after the stock entry:

```tsx
      { label: useTranslateTabs("partners.title"), component: (isActive: boolean) => <PartnersPanel isActive={isActive} />, id: "partners" },
      { label: useTranslateTabs("timeline.title"), component: (isActive: boolean) => <TimelinePanel isActive={isActive} />, id: "timeline" },
```

- [ ] **Step 4: Build**

Run: `python3 scripts/check-rpc-commands.py && (cd web && pnpm build)`
Expected: server and web counts equal, `0 missing`; tsc and vite clean, no new warnings.

- [ ] **Step 5: Commit and push**

```bash
git add web/src/pages/trading_analytics web/src/utils/chartjs.ts
git commit -m "feat(web): add Trading partners and Profit timeline tabs to Trading Analytics

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 6: Deploy and record acceptance (needs the user)

**Files:**
- Create: `docs/PHASE-6A-ACCEPTANCE.md`

- [ ] **Step 1: Ask the user for the go-ahead to deploy** (always required for ockohome). Show the commit and the rsync dry-run deletion list first. Procedure as in `docs/PHASE-4D-ACCEPTANCE.md`: rsync the worktree to `christopher@ockohome:~/stacks/quantframe-server/` excluding `.git target web/node_modules web/dist secrets .env .superpowers backups`, then `docker compose up -d --build`, confirm healthy, `Database ready`, no panic, no `CRITICAL`, no `Trader started` at boot.

- [ ] **Step 2: Run the A9 checks with the user in the browser:**

| # | Check | How |
|---|---|---|
| 1 | Items P&L for the full range sums revenue, spend and profit to the Transaction tab's financial report for the same range | Set both ranges to 2026-01-01 → today; compare the three totals |
| 2 | Galvanized Shot shows 3 purchases and 2 sales | Search the Items tab; the row after the user's row-283 deletion |
| 3 | Stock performance lists the 16 stock rows; tracked ones show market fields | Count rows; at least one row has a median |
| 4 | Trading partners: one spot-checked user has the right counts | Pick a user from the Transaction tab, filter there, compare |
| 5 | Timeline monthly totals match the Home page's yearly bar chart | Week buckets for this year; sum profit per month against the Home chart's bars |

- [ ] **Step 3: Write `docs/PHASE-6A-ACCEPTANCE.md`** in the shape of `docs/PHASE-4D-ACCEPTANCE.md`: header (server, branch, commit, deploy time), local gate numbers, deploy evidence, the table with results, rulings made during execution, follow-ups.

- [ ] **Step 4: Commit and push**

```bash
git add docs/PHASE-6A-ACCEPTANCE.md
git commit -m "docs: record phase 6a trading analytics acceptance

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

The merge into `main` is the controller's, after the gate is green on the branch tip.

---

## Self-review

- **Spec coverage.** A1 → Task 4 Step 3 (hook) and the `RANGE` constant in Task 1; A2 → Tasks 1, 4; A3 → Tasks 2, 4; A4 → Tasks 1, 5; A5 → Tasks 1, 5; A6 → Tasks 3–5 (module names, tab ids, string keys); A7 → nothing built; A8 → Task 1 (items with two purchases and a sale, `to` exclusive, week across a month boundary — the week test spans 08-31 → 09-07), Task 1 (partner both directions), Task 3 (rpc), Tasks 4–5 (script + build); A9 → Task 6.
- **Placeholders.** None; the only conditional instructions name the exact alternative to apply (`CAST`, date value conversion, chart controller registration).
- **Type consistency.** `ItemRow`/`PartnerRow`/`TimelineRow`/`StockRow` fields match the TypeScript interfaces field for field; `Bucket` serialises as `day`/`week`, matching `AnalyticsBucket`; `sortRows` is exported from `Tabs/Items` and imported by Stock and Partners; `useAnalyticsRange` returns `{ range, setRange, from, to }` and every tab uses exactly those.
