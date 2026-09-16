# Phase 6b: Market Data — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task with Opus implementers and a reviewer between tasks. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Three new Market Data tabs — Overview, Movers, Warm-up — and recent trades plus the order-book snapshot on Price History, all read from the collector tables that already exist.

**Architecture:** A new `commands/market.rs` exposes `market_overview`, `market_movers` and `market_warmup`; their SQL lives in a new `collector/market.rs` beside the existing history queries. The Price History extension adds two queries to `collector/history.rs` and two fields to its payload. The web adds an `api/market` module and three tab components; the overview is sorted and filtered in the browser with `mantine-datatable`, and charts use `chart.js` through `react-chartjs-2`.

**Tech Stack:** Rust (sea-orm raw SQL over SQLite, serde), React 19 + Mantine 9 + mantine-datatable 8 + react-chartjs-2 + TanStack Query 5, pnpm 11.3.0.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md` — read **§23 (M1–M8)** first; §5.5 for the stats fields; §15 B9/B11 for how the daily rollups are produced. §23 takes precedence.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-phase-6b`, branch `phase-6b-market-data`, created from `main` after phase 6a is merged. Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target`.
- **Tests:** `cargo test -p utils --lib`, `cargo test -p qf_core --lib`, `cargo test -p qf-server`; never bare `--workspace`. Web: `python3 scripts/check-rpc-commands.py` and `(cd web && pnpm build)`. Output pristine. The check script's `missing` counts web calls without a server command; a server command with no web caller shows as the "used by web" count lagging the server count.
- **Exact names and values:** RPCs `market_overview {}`, `market_movers { min_volume: f64 }` (default `3.0` in the web), `market_warmup {}`; movers return at most 25 `up` and 25 `down` per period; warm-up projects the next 7 UTC dates starting tomorrow with `history_days + d >= 7 AND volume * 7 >= 10`; histogram buckets `0`…`6`, `7+` and `0`, `1-4`, `5-9`, `10+`; Price History gains `trades` (newest 50, `status = 'trade'`) and `book`; tab order Collector, Overview, Movers, Warm-up, Price History with ids `overview`, `movers`, `warmup`; `localStorage` key `market_data_price_history_selection` holding `{ slug, sub_type }`; en.json keys under `pages.market_data.tabs.{overview,movers,warmup}` and `pages.market_data.tabs.price_history.{book_title,trades_title,side,platinum,quantity,vanished_at}`.
- **`en.json`** by targeted insertion only (Python: load, insert under the existing key, dump with `indent=2, ensure_ascii=False`, trailing newline); confirm it parses and the diff touches only the new keys.
- **Commits:** conventional commits ending with exactly `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`; `git push` after each task. Never touch `main`.
- Tasks 1–5 do not touch ockohome; Task 6 does and asks the user first.

## File Structure (end of phase 6b)

```
crates/qf_core/src/collector/market.rs                 NEW  overview, movers, warmup SQL + structs + tests
crates/qf_core/src/collector/mod.rs                    MOD  pub mod market;
crates/qf_core/src/collector/history.rs                MOD  TradePoint, BookSnapshot, two queries, test
crates/qf_core/src/commands/market.rs                  NEW  three commands
crates/qf_core/src/commands/collector.rs               MOD  MarketItemHistory gains trades, book
crates/qf_core/src/commands/mod.rs                     MOD  pub mod market;
crates/qf_core/src/commands/rpc.rs                     MOD  three rows + test
web/src/api/market/index.ts                            NEW  MarketModule
web/src/api/index.ts                                   MOD  register market
web/src/types/tauri.type.ts                            MOD  Market* types; MarketItemHistory gains trades, book
web/src/pages/market_data/selection.ts                 NEW  price-history selection helpers
web/src/pages/market_data/Tabs/Overview/index.tsx      NEW
web/src/pages/market_data/Tabs/Movers/index.tsx        NEW
web/src/pages/market_data/Tabs/Warmup/index.tsx        NEW
web/src/pages/market_data/Tabs/PriceHistory/index.tsx  MOD  selection, trades, book
web/src/pages/market_data/Tabs/index.ts                MOD  exports
web/src/pages/market_data/index.tsx                    MOD  tab order
web/public/lang/en.json                                MOD  strings
docs/PHASE-6B-ACCEPTANCE.md                            NEW  (Task 6)
```

---

### Task 1: `collector::market` — overview and warm-up

**Files:**
- Create: `crates/qf_core/src/collector/market.rs`
- Modify: `crates/qf_core/src/collector/mod.rs` (add `pub mod market;`)

**Interfaces:**
- Consumes: `crate::trader::price_source::all_item_stats(conn) -> Result<Vec<ItemStats>, Error>` (fields `item_id, sub_type, volume: f64, avg_price, moving_avg, profit, min_price, max_price, median, history_days: i64, warm: bool, updated_at`); `crate::collector::store::tests::setup()` for a migrated test database and `crate::collector::store::exec`.
- Produces:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverviewRow { pub item_id: String, pub name: String, pub slug: String, pub sub_type: String, pub volume: f64, pub avg_price: Option<f64>, pub moving_avg: Option<f64>, pub median: Option<f64>, pub profit: Option<f64>, pub min_price: Option<i64>, pub max_price: Option<i64>, pub history_days: i64, pub warm: bool, pub updated_at: String }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Projection { pub date: String, pub warm_count: i64 }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistogramBucket { pub bucket: String, pub count: i64 }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Warmup { pub tracked: i64, pub warm: i64, pub projected: Vec<Projection>, pub history_days_histogram: Vec<HistogramBucket>, pub trades_histogram: Vec<HistogramBucket> }
pub fn overview(stats: Vec<ItemStats>, name_of: impl Fn(&str) -> Option<(String, String)>) -> Vec<OverviewRow>;   // name_of(item_id) -> (name, slug)
pub fn warmup(stats: &[ItemStats], today: NaiveDate) -> Warmup;
```

Both are pure functions over the stats vector so the tests need no database; the commands (Task 3) load the stats with `all_item_stats`.

- [ ] **Step 1: Write the failing tests** in `collector/market.rs`:

```rust
//! Market Data aggregates over the collector tables (spec §23 M1–M3).

use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use super::stats::ItemStats;

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(id: &str, volume: f64, history_days: i64, warm: bool) -> ItemStats {
        ItemStats {
            item_id: id.into(), sub_type: String::new(), volume, avg_price: Some(10.0), moving_avg: Some(10.0), profit: Some(2.0),
            min_price: Some(8), max_price: Some(12), median: Some(10.0), history_days, warm, updated_at: "2026-09-16T00:00:00Z".into(),
        }
    }

    #[test]
    fn overview_joins_names_and_skips_items_missing_from_the_cache() {
        let rows = overview(vec![stat("a", 1.0, 3, false), stat("gone", 1.0, 3, false)], |id| (id == "a").then(|| ("Item A".to_string(), "item_a".to_string())));
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].name.as_str(), rows[0].slug.as_str(), rows[0].history_days, rows[0].warm), ("Item A", "item_a", 3, false));
    }

    #[test]
    fn warmup_counts_projects_and_buckets() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 16).unwrap();
        let stats = vec![
            stat("warm", 2.0, 9, true),      // 14 trades, already warm
            stat("soon", 12.0 / 7.0, 5, false), // 12 trades, warm when history_days reaches 7: d = 2 → 2026-09-18
            stat("thin", 4.0 / 7.0, 5, false),  // 4 trades: never in this projection
            stat("late", 10.0 / 7.0, 1, false), // 10 trades, warm at d = 6 → 2026-09-22
        ];
        let w = warmup(&stats, today);
        assert_eq!((w.tracked, w.warm), (4, 1));
        assert_eq!(w.projected.len(), 7);
        assert_eq!(w.projected[0], Projection { date: "2026-09-17".into(), warm_count: 1 });
        assert_eq!(w.projected[1], Projection { date: "2026-09-18".into(), warm_count: 2 });
        assert_eq!(w.projected[5], Projection { date: "2026-09-22".into(), warm_count: 3 });
        assert_eq!(w.projected[6].warm_count, 3, "thin never qualifies");
        assert_eq!(
            w.history_days_histogram.iter().filter(|b| b.count > 0).map(|b| (b.bucket.as_str(), b.count)).collect::<Vec<_>>(),
            vec![("1", 1), ("5", 2), ("7+", 1)]
        );
        assert_eq!(
            w.trades_histogram,
            vec![
                HistogramBucket { bucket: "0".into(), count: 0 },
                HistogramBucket { bucket: "1-4".into(), count: 1 },
                HistogramBucket { bucket: "5-9".into(), count: 0 },
                HistogramBucket { bucket: "10+".into(), count: 3 },
            ]
        );
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib collector::market`
Expected: compile errors (`overview`, `warmup`, structs missing). Add `pub mod market;` to `collector/mod.rs` first so the module is compiled.

- [ ] **Step 3: Implement** above the tests in `collector/market.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverviewRow {
    pub item_id: String,
    pub name: String,
    pub slug: String,
    pub sub_type: String,
    pub volume: f64,
    pub avg_price: Option<f64>,
    pub moving_avg: Option<f64>,
    pub median: Option<f64>,
    pub profit: Option<f64>,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
    pub history_days: i64,
    pub warm: bool,
    pub updated_at: String,
}

/// Every stats row with its cache name and slug; rows whose item is not in the cache are skipped (spec §23 M1).
pub fn overview(stats: Vec<ItemStats>, name_of: impl Fn(&str) -> Option<(String, String)>) -> Vec<OverviewRow> {
    stats
        .into_iter()
        .filter_map(|s| {
            let (name, slug) = name_of(&s.item_id)?;
            Some(OverviewRow {
                item_id: s.item_id,
                name,
                slug,
                sub_type: s.sub_type,
                volume: s.volume,
                avg_price: s.avg_price,
                moving_avg: s.moving_avg,
                median: s.median,
                profit: s.profit,
                min_price: s.min_price,
                max_price: s.max_price,
                history_days: s.history_days,
                warm: s.warm,
                updated_at: s.updated_at,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Projection {
    pub date: String,
    pub warm_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistogramBucket {
    pub bucket: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Warmup {
    pub tracked: i64,
    pub warm: i64,
    pub projected: Vec<Projection>,
    pub history_days_histogram: Vec<HistogramBucket>,
    pub trades_histogram: Vec<HistogramBucket>,
}

const WARM_DAYS: i64 = 7;
const WARM_TRADES: f64 = 10.0;

/// Warm counts, a 7-day projection with trade counts held constant, and two histograms (spec §23 M3).
pub fn warmup(stats: &[ItemStats], today: NaiveDate) -> Warmup {
    let trades7 = |s: &ItemStats| (s.volume * 7.0).round();
    let projected = (1..=7)
        .map(|d| Projection {
            date: (today + Duration::days(d)).to_string(),
            warm_count: stats.iter().filter(|s| s.history_days + d >= WARM_DAYS && trades7(s) >= WARM_TRADES).count() as i64,
        })
        .collect();
    let history_days_histogram = (0..=7)
        .map(|day| HistogramBucket {
            bucket: if day == 7 { "7+".into() } else { day.to_string() },
            count: stats.iter().filter(|s| if day == 7 { s.history_days >= 7 } else { s.history_days == day }).count() as i64,
        })
        .collect();
    let trades_histogram = [("0", 0.0, 0.0), ("1-4", 1.0, 4.0), ("5-9", 5.0, 9.0), ("10+", 10.0, f64::INFINITY)]
        .into_iter()
        .map(|(bucket, lo, hi)| HistogramBucket {
            bucket: bucket.into(),
            count: stats.iter().filter(|s| (lo..=hi).contains(&trades7(s))).count() as i64,
        })
        .collect();
    Warmup {
        tracked: stats.len() as i64,
        warm: stats.iter().filter(|s| s.warm).count() as i64,
        projected,
        history_days_histogram,
        trades_histogram,
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib collector::market`
Expected: 2 passed.

- [ ] **Step 5: Commit and push**

```bash
git add crates/qf_core/src/collector/market.rs crates/qf_core/src/collector/mod.rs
git commit -m "feat(collector): add market overview rows and warm-up projection

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push -u origin phase-6b-market-data
```

---

### Task 2: `collector::market::movers` — median change over 1 and 7 days

**Files:**
- Modify: `crates/qf_core/src/collector/market.rs`

**Interfaces:**
- Consumes: `super::{db_err, stmt}`, `super::store::exec` (tests), `super::store::tests::setup()`.
- Produces:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Mover { pub item_id: String, pub name: String, pub slug: String, pub sub_type: String, pub median_now: f64, pub median_then: f64, pub change_pct: f64, pub volume: f64 }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MoverList { pub up: Vec<Mover>, pub down: Vec<Mover> }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Movers { pub day: MoverList, pub week: MoverList }
pub async fn movers(conn: &DatabaseConnection, min_volume: f64, name_of: impl Fn(&str) -> Option<(String, String)>) -> Result<Movers, Error>;
```

- [ ] **Step 1: Write the failing test** in the `tests` module of `collector/market.rs`:

```rust
    async fn daily(conn: &DatabaseConnection, item: &str, day: &str, median: Option<f64>) {
        exec(conn, "Test:Daily", "INSERT INTO item_stats_daily (item_id, sub_type, day, volume, median, min_price, max_price) VALUES (?, '', ?, 5, ?, NULL, NULL)", vec![item.into(), day.into(), median.into()]).await.unwrap();
    }
    async fn current(conn: &DatabaseConnection, item: &str, volume: f64) {
        exec(conn, "Test:Stats", "INSERT INTO item_stats (item_id, sub_type, volume, history_days, warm, updated_at) VALUES (?, '', ?, 3, 0, '2026-09-16T00:00:00Z')", vec![item.into(), volume.into()]).await.unwrap();
    }

    #[tokio::test]
    async fn movers_compare_the_latest_median_with_one_and_seven_days_earlier_and_drop_thin_items() {
        use crate::collector::store::{exec, tests::setup};
        let (_dir, conn) = setup().await;
        // "rise": 100 → 110 (day) and 80 → 110 (week). The 09-15 row has a NULL median, so the day comparison falls back to 09-14.
        for (day, median) in [("2026-09-08", Some(80.0)), ("2026-09-14", Some(100.0)), ("2026-09-15", None), ("2026-09-16", Some(110.0))] {
            daily(&conn, "rise", day, median).await;
        }
        current(&conn, "rise", 5.0).await;
        // "fall": 50 → 40 over a day; only two days of history, so no week comparison.
        daily(&conn, "fall", "2026-09-15", Some(50.0)).await;
        daily(&conn, "fall", "2026-09-16", Some(40.0)).await;
        current(&conn, "fall", 5.0).await;
        // "thin": huge move but volume below the threshold.
        daily(&conn, "thin", "2026-09-15", Some(10.0)).await;
        daily(&conn, "thin", "2026-09-16", Some(30.0)).await;
        current(&conn, "thin", 1.0).await;

        let m = movers(&conn, 3.0, |id| Some((id.to_uppercase(), id.to_string()))).await.unwrap();
        assert_eq!(m.day.up.iter().map(|x| x.item_id.as_str()).collect::<Vec<_>>(), vec!["rise"]);
        assert!((m.day.up[0].change_pct - 10.0).abs() < 1e-9);
        assert_eq!((m.day.up[0].median_then, m.day.up[0].name.as_str()), (100.0, "RISE"));
        assert_eq!(m.day.down.iter().map(|x| x.item_id.as_str()).collect::<Vec<_>>(), vec!["fall"]);
        assert!((m.day.down[0].change_pct + 20.0).abs() < 1e-9);
        assert_eq!(m.week.up.iter().map(|x| x.item_id.as_str()).collect::<Vec<_>>(), vec!["rise"]);
        assert!((m.week.up[0].change_pct - 37.5).abs() < 1e-9);
        assert!(m.week.down.is_empty(), "fall has no row seven days back");
    }
```

Move the `use crate::collector::store::{exec, tests::setup};` line to the top of the `tests` module so the helper functions can use `exec`. Check the exact column list of `item_stats` in `crates/migration/src/m20260915_000001_create_collector_tables.rs` (`avg_price`, `moving_avg`, `profit`, `min_price`, `max_price`, `median` are nullable, so the INSERT above is valid) before running.

- [ ] **Step 2: Run it to verify it fails**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib collector::market::tests::movers`
Expected: compile error, `movers` not found.

- [ ] **Step 3: Implement** (add `use service::sea_orm::{ConnectionTrait, DatabaseConnection};`, `use utils::Error;`, `use super::{db_err, stmt};`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Mover {
    pub item_id: String,
    pub name: String,
    pub slug: String,
    pub sub_type: String,
    pub median_now: f64,
    pub median_then: f64,
    pub change_pct: f64,
    pub volume: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MoverList {
    pub up: Vec<Mover>,
    pub down: Vec<Mover>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Movers {
    pub day: MoverList,
    pub week: MoverList,
}

const MOVERS_PER_SIDE: usize = 25;

/// Latest daily median against the latest median at or before `days_back` days earlier (spec §23 M2).
async fn movers_for(conn: &DatabaseConnection, days_back: i64, min_volume: f64, name_of: &impl Fn(&str) -> Option<(String, String)>) -> Result<MoverList, Error> {
    const C: &str = "Market:Movers";
    let offset = format!("-{days_back} days");
    let rows = conn
        .query_all(stmt(
            "WITH latest AS (
                SELECT item_id, sub_type, MAX(day) AS day FROM item_stats_daily WHERE median IS NOT NULL GROUP BY item_id, sub_type
             ),
             now AS (
                SELECT d.item_id, d.sub_type, d.day, d.median FROM item_stats_daily d
                JOIN latest l ON l.item_id = d.item_id AND l.sub_type = d.sub_type AND l.day = d.day
             )
             SELECT n.item_id, n.sub_type, n.median AS median_now, s.volume,
                    (SELECT p.median FROM item_stats_daily p
                      WHERE p.item_id = n.item_id AND p.sub_type = n.sub_type AND p.median IS NOT NULL AND p.day <= date(n.day, ?)
                      ORDER BY p.day DESC LIMIT 1) AS median_then
             FROM now n JOIN item_stats s ON s.item_id = n.item_id AND s.sub_type = n.sub_type
             WHERE s.volume >= ?",
            vec![offset.into(), min_volume.into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?;
    let mut all = Vec::new();
    for r in rows.iter() {
        let median_then: Option<f64> = r.try_get("", "median_then").map_err(|e| db_err(C, e))?;
        let Some(median_then) = median_then.filter(|m| *m > 0.0) else { continue };
        let item_id: String = r.try_get("", "item_id").map_err(|e| db_err(C, e))?;
        let Some((name, slug)) = name_of(&item_id) else { continue };
        let median_now: f64 = r.try_get("", "median_now").map_err(|e| db_err(C, e))?;
        all.push(Mover {
            item_id,
            name,
            slug,
            sub_type: r.try_get("", "sub_type").map_err(|e| db_err(C, e))?,
            median_now,
            median_then,
            change_pct: (median_now - median_then) / median_then * 100.0,
            volume: r.try_get("", "volume").map_err(|e| db_err(C, e))?,
        });
    }
    all.sort_by(|a, b| b.change_pct.partial_cmp(&a.change_pct).unwrap_or(std::cmp::Ordering::Equal));
    let up = all.iter().filter(|m| m.change_pct > 0.0).take(MOVERS_PER_SIDE).cloned().collect();
    let down = all.iter().rev().filter(|m| m.change_pct < 0.0).take(MOVERS_PER_SIDE).cloned().collect();
    Ok(MoverList { up, down })
}

pub async fn movers(conn: &DatabaseConnection, min_volume: f64, name_of: impl Fn(&str) -> Option<(String, String)>) -> Result<Movers, Error> {
    Ok(Movers { day: movers_for(conn, 1, min_volume, &name_of).await?, week: movers_for(conn, 7, min_volume, &name_of).await? })
}
```

- [ ] **Step 4: Run the tests**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib collector::market`
Expected: 3 passed.

- [ ] **Step 5: Commit and push**

```bash
git add crates/qf_core/src/collector/market.rs
git commit -m "feat(collector): add top movers over one and seven days

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 3: Price History extension and the three RPC commands

**Files:**
- Modify: `crates/qf_core/src/collector/history.rs` (structs, two queries, test), `crates/qf_core/src/commands/collector.rs` (`MarketItemHistory` fields), `crates/qf_core/src/commands/mod.rs`, `crates/qf_core/src/commands/rpc.rs`
- Create: `crates/qf_core/src/commands/market.rs`

**Interfaces:**
- Consumes: Tasks 1–2; `crate::trader::price_source::all_item_stats`; `states::cache_client()?.tradable_item()` with `get_by(item_id) -> Result<CacheTradableItem, Error>` (fields `name`, `wfm_url`).
- Produces: `History` and `MarketItemHistory` gain `trades: Vec<TradePoint>` and `book: Option<BookSnapshot>`; RPCs `market_overview {}`, `market_movers { min_volume: f64 }`, `market_warmup {}`.

- [ ] **Step 1: Write the failing history test** in `collector/history.rs` `tests`, after the existing test:

```rust
    #[tokio::test]
    async fn history_returns_recent_trades_newest_first_and_the_latest_book() {
        use crate::collector::store::exec;
        let (_dir, conn) = setup().await;
        // Two sweeps of the same sell order at 20 then its disappearance produce one probable trade via the collector's own diff.
        sweep(&conn, "item1", &[order("s1", "sell", 20, 1, 0, "u1"), order("s2", "sell", 25, 1, 0, "u2")], 0, 300).await;
        sweep(&conn, "item1", &[order("s2", "sell", 25, 1, 0, "u2")], 5, 300).await;
        // Force the vanished row to 'trade' regardless of the resolver's timing rules, plus 51 older synthetic trades to prove the cap.
        exec(&conn, "Test:Trade", "UPDATE vanished_orders SET status = 'trade'", vec![]).await.unwrap();
        for i in 0..51 {
            exec(
                &conn,
                "Test:Trade",
                "INSERT INTO vanished_orders (order_id, item_id, sub_type, side, platinum, quantity, user_id, first_seen, vanished_at, gap_seconds, kind, status)
                 VALUES (?, 'item1', 'rank=0', 'buy', 7, 1, 'u9', '2026-09-01T00:00:00Z', ?, 60, 'full', 'trade')",
                vec![format!("old{i}").into(), format!("2026-09-0{}T00:{:02}:00Z", 1 + i / 60, i % 60).into()],
            )
            .await
            .unwrap();
        }
        let history = load_history(&conn, "item1", None, 7, at(70)).await.unwrap();
        assert_eq!(history.trades.len(), 50);
        assert_eq!((history.trades[0].side.as_str(), history.trades[0].platinum), ("sell", 20), "the real vanished sell is the newest");
        assert!(history.trades.windows(2).all(|w| w[0].vanished_at >= w[1].vanished_at));
        let book = history.book.expect("latest sweep row");
        assert_eq!(book.top_sells, vec![[25, 1]]);
        assert!(book.top_buys.is_empty());
        assert_eq!(book.swept_at, "2026-09-15T00:05:00Z");
    }
```

Read the existing test's helpers (`setup`, `sweep`, `order`, `at` in `collector/store.rs` tests) to confirm `sweep(conn, item, orders, minute, interval)` stores `top_sells` from the in-game sells and `at(n)` is `2026-09-15T00:00:00Z + n minutes`; adjust the expected `swept_at` and sub-type (`rank=0` is what `order(.., rank 0, ..)` produces) to what those helpers actually write, and say so in your report.

- [ ] **Step 2: Run it to verify it fails**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib collector::history::tests::history_returns_recent`
Expected: compile error, no field `trades` on `History`.

- [ ] **Step 3: Extend `History`** in `collector/history.rs`:

```rust
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TradePoint {
    pub vanished_at: String,
    pub side: String,
    pub platinum: i64,
    pub quantity: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BookSnapshot {
    pub swept_at: String,
    pub top_sells: Vec<[i64; 2]>,
    pub top_buys: Vec<[i64; 2]>,
}
```

add `pub trades: Vec<TradePoint>, pub book: Option<BookSnapshot>,` to `History`, and before the final `Ok(History { ... })` in `load_history`:

```rust
    let trades = conn
        .query_all(stmt(
            "SELECT vanished_at, side, platinum, quantity FROM vanished_orders
             WHERE item_id = ? AND sub_type = ? AND status = 'trade' ORDER BY vanished_at DESC, id DESC LIMIT 50",
            vec![item_id.into(), chosen.clone().into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            Ok(TradePoint {
                vanished_at: r.try_get("", "vanished_at").map_err(|e| db_err(C, e))?,
                side: r.try_get("", "side").map_err(|e| db_err(C, e))?,
                platinum: r.try_get("", "platinum").map_err(|e| db_err(C, e))?,
                quantity: r.try_get("", "quantity").map_err(|e| db_err(C, e))?,
            })
        })
        .collect::<Result<_, Error>>()?;

    let book = conn
        .query_one(stmt(
            "SELECT swept_at, top_sells, top_buys FROM sweep_summary WHERE item_id = ? AND sub_type = ? ORDER BY swept_at DESC, id DESC LIMIT 1",
            vec![item_id.into(), chosen.clone().into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .map(|r| -> Result<BookSnapshot, Error> {
            let sells: String = r.try_get("", "top_sells").map_err(|e| db_err(C, e))?;
            let buys: String = r.try_get("", "top_buys").map_err(|e| db_err(C, e))?;
            Ok(BookSnapshot {
                swept_at: r.try_get("", "swept_at").map_err(|e| db_err(C, e))?,
                top_sells: serde_json::from_str(&sells).map_err(|e| db_err(C, e))?,
                top_buys: serde_json::from_str(&buys).map_err(|e| db_err(C, e))?,
            })
        })
        .transpose()?;
```

and include `trades, book` in the returned struct. In `commands/collector.rs` add the same two fields to `MarketItemHistory` (importing `BookSnapshot, TradePoint` from `crate::collector::history`) and copy them across in `market_item_history`.

- [ ] **Step 4: Run the history tests**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib collector::history`
Expected: all pass, including the new one.

- [ ] **Step 5: Write the failing rpc test** in `rpc.rs` `tests`:

```rust
    #[tokio::test]
    async fn market_commands_are_routable_and_validate_args() {
        for name in ["market_overview", "market_movers", "market_warmup"] {
            assert!(COMMANDS.contains(&name), "{name}");
        }
        assert!(dispatch("market_movers", json!({"minVolume": "three"})).await.unwrap().is_err(), "min_volume must be a number");
    }
```

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib commands::rpc::tests::market_commands`
Expected: FAIL, `market_overview` not in `COMMANDS`.

- [ ] **Step 6: Write the command module** `commands/market.rs`:

```rust
//! Market Data RPCs (spec §23 M1–M3).

use chrono::Utc;
use utils::{get_location, Error};

use crate::collector::market::{self, Movers, OverviewRow, Warmup};
use crate::trader::price_source::all_item_stats;
use crate::utils::modules::states;
use crate::DATABASE;

fn database() -> Result<&'static service::sea_orm::DatabaseConnection, Error> {
    DATABASE.get().ok_or_else(|| Error::new("Market:Rpc", "Database is not ready", get_location!()))
}

/// `(name, slug)` for a cache item id, or None when the item is no longer tradable.
fn name_of() -> Result<impl Fn(&str) -> Option<(String, String)>, Error> {
    let tradable = states::cache_client()?.tradable_item();
    Ok(move |id: &str| tradable.get_by(id).ok().map(|item| (item.name, item.wfm_url)))
}

pub async fn market_overview() -> Result<Vec<OverviewRow>, Error> {
    let stats = all_item_stats(database()?).await?;
    Ok(market::overview(stats, name_of()?))
}

pub async fn market_movers(min_volume: f64) -> Result<Movers, Error> {
    market::movers(database()?, min_volume, name_of()?).await
}

pub async fn market_warmup() -> Result<Warmup, Error> {
    let stats = all_item_stats(database()?).await?;
    Ok(market::warmup(&stats, Utc::now().date_naive()))
}
```

If `tradable_item()` returns a reference tied to the cache client's lifetime so the closure cannot own it, clone the client (`let cache = states::cache_client()?;` then `move |id| cache.tradable_item().get_by(id)...`) — `cache_client()` returns an owned handle elsewhere in `commands/trader.rs`, so follow that.

Add `pub mod market;` to `commands/mod.rs` and the rows to `rpc.rs` after `market_item_history`:

```rust
    market_overview => market::market_overview {},
    market_movers => market::market_movers { min_volume: f64 },
    market_warmup => market::market_warmup {},
```

- [ ] **Step 7: Run the suite and the script**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib && python3 scripts/check-rpc-commands.py`
Expected: all pass; the script's "used by web" count is 3 below the server count (Tasks 4–5 close it), `0 missing`.

- [ ] **Step 8: Commit and push**

```bash
git add crates/qf_core/src/collector/history.rs crates/qf_core/src/commands
git commit -m "feat(rpc): add market_overview, market_movers, market_warmup and the price-history trades and book

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 4: Web — API module, types, Overview and Warm-up tabs, selection hand-off

**Files:**
- Create: `web/src/api/market/index.ts`, `web/src/pages/market_data/selection.ts`, `web/src/pages/market_data/Tabs/Overview/index.tsx`, `web/src/pages/market_data/Tabs/Warmup/index.tsx`
- Modify: `web/src/api/index.ts`, `web/src/types/tauri.type.ts`, `web/src/pages/market_data/Tabs/index.ts`, `web/src/pages/market_data/index.tsx`, `web/src/pages/market_data/Tabs/PriceHistory/index.tsx` (read the selection), `web/public/lang/en.json`

**Interfaces:**
- Consumes: Task 3 RPCs and payload shapes.
- Produces: `api.market.{overview(), movers(minVolume), warmup()}`; `readSelection()`/`writeSelection()` in `selection.ts`; tabs `overview`, `warmup`.

- [ ] **Step 1: Types** in `tauri.type.ts` (after the `Analytics*` types from 6a; also add `trades` and `book` to `MarketItemHistory`):

```ts
  export interface MarketOverviewRow {
    item_id: string;
    name: string;
    slug: string;
    sub_type: string;
    volume: number;
    avg_price?: number | null;
    moving_avg?: number | null;
    median?: number | null;
    profit?: number | null;
    min_price?: number | null;
    max_price?: number | null;
    history_days: number;
    warm: boolean;
    updated_at: string;
  }
  export interface MarketMover {
    item_id: string;
    name: string;
    slug: string;
    sub_type: string;
    median_now: number;
    median_then: number;
    change_pct: number;
    volume: number;
  }
  export interface MarketMoverList {
    up: MarketMover[];
    down: MarketMover[];
  }
  export interface MarketMovers {
    day: MarketMoverList;
    week: MarketMoverList;
  }
  export interface MarketProjection {
    date: string;
    warm_count: number;
  }
  export interface MarketHistogramBucket {
    bucket: string;
    count: number;
  }
  export interface MarketWarmup {
    tracked: number;
    warm: number;
    projected: MarketProjection[];
    history_days_histogram: MarketHistogramBucket[];
    trades_histogram: MarketHistogramBucket[];
  }
  export interface MarketTradePoint {
    vanished_at: string;
    side: "buy" | "sell";
    platinum: number;
    quantity: number;
  }
  export interface MarketBookSnapshot {
    swept_at: string;
    top_sells: [number, number][];
    top_buys: [number, number][];
  }
```

In `MarketItemHistory` add `trades: MarketTradePoint[];` and `book?: MarketBookSnapshot | null;`.

- [ ] **Step 2: API module** `web/src/api/market/index.ts`:

```ts
import { TauriClient } from "..";
import { TauriTypes } from "$types";

export class MarketModule {
  constructor(private readonly client: TauriClient) {}

  overview() {
    return this.client.sendInvoke<TauriTypes.MarketOverviewRow[]>("market_overview");
  }
  movers(minVolume: number) {
    return this.client.sendInvoke<TauriTypes.MarketMovers>("market_movers", { minVolume });
  }
  warmup() {
    return this.client.sendInvoke<TauriTypes.MarketWarmup>("market_warmup");
  }
}
```

Register in `web/src/api/index.ts` like the other modules (`market: MarketModule;`, `this.market = new MarketModule(this);`).

- [ ] **Step 3: Selection helpers** `web/src/pages/market_data/selection.ts`:

```ts
const KEY = "market_data_price_history_selection";

export interface PriceHistorySelection {
  slug: string;
  sub_type: string;
}

export function readSelection(): PriceHistorySelection | null {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? (JSON.parse(raw) as PriceHistorySelection) : null;
  } catch {
    return null;
  }
}

export function writeSelection(selection: PriceHistorySelection) {
  try {
    localStorage.setItem(KEY, JSON.stringify(selection));
  } catch {
    /* storage unavailable: the click still switches tabs */
  }
}
```

- [ ] **Step 4: Strings.** Insert with a script:

```bash
cd web && python3 - <<'EOF'
import json
p = "public/lang/en.json"
d = json.load(open(p, encoding="utf-8"))
tabs = d["pages"]["market_data"]["tabs"]
for key in ("overview", "movers", "warmup"):
    assert key not in tabs
tabs["overview"] = {
  "title": "Overview",
  "search": "Search items",
  "warm_only": "Warm only",
  "min_volume": "Min trades / day",
  "columns": {"item": "Item", "sub_type": "Rank / variant", "volume": "Trades / day", "median": "Median (7 d)", "moving_avg": "Moving avg", "profit": "Spread", "min_price": "Min", "max_price": "Max", "history_days": "History (d)", "warm": "Warm", "updated_at": "Updated"},
  "rows": "{{shown}} of {{total}} items",
  "empty": "No items match."
}
tabs["movers"] = {
  "title": "Movers",
  "period": {"day": "24 h", "week": "7 d"},
  "min_volume": "Min trades / day",
  "up": "Rising",
  "down": "Falling",
  "columns": {"item": "Item", "sub_type": "Rank / variant", "median_then": "Was", "median_now": "Now", "change_pct": "Change", "volume": "Trades / day"},
  "empty": "Nothing moved."
}
tabs["warmup"] = {
  "title": "Warm-up",
  "tracked": "Tracked items",
  "warm": "Warm now",
  "projected_title": "Projected warm items by date (trade counts held constant)",
  "history_title": "Days of history",
  "trades_title": "Trades in the last 7 days",
  "warm_count": "Warm items",
  "items": "Items"
}
ph = tabs["price_history"]
for key, value in {"book_title": "Latest order book", "trades_title": "Recent probable trades", "side": "Side", "platinum": "Platinum", "quantity": "Qty", "vanished_at": "Traded at", "sells": "Sells", "buys": "Buys", "no_book": "No sweep recorded yet.", "no_trades": "No probable trades yet."}.items():
    assert key not in ph
    ph[key] = value
with open(p, "w", encoding="utf-8") as f:
    json.dump(d, f, indent=2, ensure_ascii=False)
    f.write("\n")
EOF
python3 -c "import json; json.load(open('public/lang/en.json', encoding='utf-8'))" && git diff --stat public/lang/en.json
```

Expected: insertions only under `market_data.tabs`. If re-serialisation touched other lines, revert and insert by text edit.

- [ ] **Step 5: Overview tab** `Tabs/Overview/index.tsx`:

```tsx
import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Badge, Group, NumberInput, Switch, Text, TextInput } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import dayjs from "dayjs";
import { DataTable, DataTableSortStatus } from "mantine-datatable";
import { useMemo, useState } from "react";
import { TauriTypes } from "$types";
import { writeSelection } from "../../selection";

const PAGE = 50;
const num = (value?: number | null, digits = 1) => (value == null ? "—" : value.toFixed(digits));

function sortRows<T>(rows: T[], status: DataTableSortStatus<T>): T[] {
  const key = status.columnAccessor as keyof T;
  const dir = status.direction === "asc" ? 1 : -1;
  return [...rows].sort((a, b) => {
    const x = a[key] as unknown, y = b[key] as unknown;
    if (x == null && y == null) return 0;
    if (x == null) return 1;
    if (y == null) return -1;
    if (typeof x === "number" && typeof y === "number") return (x - y) * dir;
    if (typeof x === "boolean" && typeof y === "boolean") return (Number(x) - Number(y)) * dir;
    return String(x).localeCompare(String(y)) * dir;
  });
}

export function OverviewPanel({ isActive, onOpenItem }: { isActive?: boolean; onOpenItem: () => void }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`market_data.tabs.overview.${key}`, context);
  const [search, setSearch] = useState("");
  const [warmOnly, setWarmOnly] = useState(false);
  const [minVolume, setMinVolume] = useState<number>(0);
  const [page, setPage] = useState(1);
  const [sort, setSort] = useState<DataTableSortStatus<TauriTypes.MarketOverviewRow>>({ columnAccessor: "profit", direction: "desc" });
  const { data, isFetching } = useQuery({ queryKey: ["market_overview"], queryFn: () => api.market.overview(), enabled: !!isActive, refetchInterval: 60_000 });
  const rows = useMemo(() => {
    const filtered = (data ?? []).filter((r) => (!warmOnly || r.warm) && r.volume >= minVolume && r.name.toLowerCase().includes(search.toLowerCase()));
    return sortRows(filtered, sort);
  }, [data, warmOnly, minVolume, search, sort]);
  const shown = rows.slice((page - 1) * PAGE, page * PAGE);

  return (
    <>
      <Group mt="md" align="end">
        <TextInput label={t("search")} value={search} onChange={(e) => { setSearch(e.currentTarget.value); setPage(1); }} w={260} />
        <NumberInput label={t("min_volume")} value={minVolume} onChange={(v) => { setMinVolume(Number(v) || 0); setPage(1); }} min={0} step={0.5} w={160} />
        <Switch label={t("warm_only")} checked={warmOnly} onChange={(e) => { setWarmOnly(e.currentTarget.checked); setPage(1); }} />
        <Text size="sm" c="dimmed">{t("rows", { shown: rows.length, total: data?.length ?? 0 })}</Text>
      </Group>
      <DataTable
        mt="md"
        striped
        highlightOnHover
        fetching={isFetching}
        records={shown}
        idAccessor={(r) => `${r.item_id}|${r.sub_type}`}
        totalRecords={rows.length}
        recordsPerPage={PAGE}
        page={page}
        onPageChange={setPage}
        sortStatus={sort}
        onSortStatusChange={(s) => { setSort(s); setPage(1); }}
        noRecordsText={t("empty")}
        onRowClick={({ record }) => { writeSelection({ slug: record.slug, sub_type: record.sub_type }); onOpenItem(); }}
        columns={[
          { accessor: "name", title: t("columns.item"), sortable: true },
          { accessor: "sub_type", title: t("columns.sub_type"), render: (r) => r.sub_type || "—" },
          { accessor: "volume", title: t("columns.volume"), sortable: true, render: (r) => num(r.volume, 2) },
          { accessor: "median", title: t("columns.median"), sortable: true, render: (r) => num(r.median) },
          { accessor: "moving_avg", title: t("columns.moving_avg"), sortable: true, render: (r) => num(r.moving_avg) },
          { accessor: "profit", title: t("columns.profit"), sortable: true, render: (r) => num(r.profit) },
          { accessor: "min_price", title: t("columns.min_price"), sortable: true, render: (r) => r.min_price ?? "—" },
          { accessor: "max_price", title: t("columns.max_price"), sortable: true, render: (r) => r.max_price ?? "—" },
          { accessor: "history_days", title: t("columns.history_days"), sortable: true },
          { accessor: "warm", title: t("columns.warm"), sortable: true, render: (r) => <Badge color={r.warm ? "green" : "gray"}>{r.warm ? "✓" : "—"}</Badge> },
          { accessor: "updated_at", title: t("columns.updated_at"), sortable: true, render: (r) => dayjs(r.updated_at).format("MM-DD HH:mm") },
        ]}
      />
    </>
  );
}
```

- [ ] **Step 6: Warm-up tab** `Tabs/Warmup/index.tsx`:

```tsx
import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Box, Paper, SimpleGrid, Stack, Text, useMantineTheme } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { Bar } from "react-chartjs-2";
import { TauriTypes } from "$types";

function Histogram({ title, label, buckets, color }: { title: string; label: string; buckets: TauriTypes.MarketHistogramBucket[]; color: string }) {
  return (
    <Paper withBorder p="sm">
      <Text fw={600} mb="xs">{title}</Text>
      <Box h={220}>
        <Bar options={{ responsive: true, maintainAspectRatio: false }} data={{ labels: buckets.map((b) => b.bucket), datasets: [{ label, data: buckets.map((b) => b.count), backgroundColor: color }] }} />
      </Box>
    </Paper>
  );
}

export function WarmupPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`market_data.tabs.warmup.${key}`, context);
  const theme = useMantineTheme();
  const { data } = useQuery({ queryKey: ["market_warmup"], queryFn: () => api.market.warmup(), enabled: !!isActive, refetchInterval: 60_000 });
  if (!data) return null;
  return (
    <Stack mt="md">
      <SimpleGrid cols={{ base: 2, md: 4 }}>
        {[[t("tracked"), data.tracked], [t("warm"), data.warm]].map(([label, value]) => (
          <Paper withBorder p="sm" key={String(label)}>
            <Text size="xs" c="dimmed">{label}</Text>
            <Text fw={700} size="lg">{value}</Text>
          </Paper>
        ))}
      </SimpleGrid>
      <Paper withBorder p="sm">
        <Text fw={600} mb="xs">{t("projected_title")}</Text>
        <Box h={260}>
          <Bar
            options={{ responsive: true, maintainAspectRatio: false }}
            data={{ labels: data.projected.map((p) => p.date), datasets: [{ label: t("warm_count"), data: data.projected.map((p) => p.warm_count), backgroundColor: theme.colors.green[6] }] }}
          />
        </Box>
      </Paper>
      <SimpleGrid cols={{ base: 1, md: 2 }}>
        <Histogram title={t("history_title")} label={t("items")} buckets={data.history_days_histogram} color={theme.colors.blue[6]} />
        <Histogram title={t("trades_title")} label={t("items")} buckets={data.trades_histogram} color={theme.colors.violet[6]} />
      </SimpleGrid>
    </Stack>
  );
}
```

- [ ] **Step 7: Wire tabs and the selection hand-off.** `Tabs/index.ts` adds `export * from "./Overview";` and `export * from "./Warmup";`. In `pages/market_data/index.tsx` build the tab list in the M5 order (Movers comes in Task 5; leave its slot for now):

```tsx
  const tabs = useMemo(
    () => [
      { id: "collector", label: useTranslateTabs("collector.title"), component: (isActive: boolean) => <CollectorPanel isActive={isActive} /> },
      { id: "overview", label: useTranslateTabs("overview.title"), component: (isActive: boolean) => <OverviewPanel isActive={isActive} onOpenItem={() => setActiveTab("price_history")} /> },
      { id: "warmup", label: useTranslateTabs("warmup.title"), component: (isActive: boolean) => <WarmupPanel isActive={isActive} /> },
      { id: "price_history", label: useTranslateTabs("price_history.title"), component: (isActive: boolean) => <PriceHistoryPanel isActive={isActive} /> },
    ],
    [],
  );
```

`setActiveTab` comes from the `useLocalStorage` call, which must therefore be declared before `tabs` (move it up; its `defaultValue` becomes the literal `"collector"`).

In `Tabs/PriceHistory/index.tsx` read the selection when the tab becomes active:

```tsx
import { useEffect, useState } from "react";
import { readSelection } from "../../selection";
// inside the component, after the useState calls:
  useEffect(() => {
    if (!isActive) return;
    const selection = readSelection();
    if (selection && selection.slug !== wfmUrl) {
      setWfmUrl(selection.slug);
      setSubType(selection.sub_type || undefined);
    }
  }, [isActive]);
```

`SelectTradableItem` is a controlled component on `value={wfmUrl}`, so setting the state selects it.

- [ ] **Step 8: Build**

Run: `python3 scripts/check-rpc-commands.py && (cd web && pnpm build)`
Expected: "used by web" is 1 below the server count (movers in Task 5); tsc and vite clean.

- [ ] **Step 9: Commit and push**

```bash
git add web/src/api/market/index.ts web/src/api/index.ts web/src/types/tauri.type.ts web/src/pages/market_data web/public/lang/en.json
git commit -m "feat(web): add Market Data Overview and Warm-up tabs with a hand-off to Price History

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 5: Web — Movers tab, trades and book on Price History

**Files:**
- Create: `web/src/pages/market_data/Tabs/Movers/index.tsx`
- Modify: `web/src/pages/market_data/Tabs/index.ts`, `web/src/pages/market_data/index.tsx`, `web/src/pages/market_data/Tabs/PriceHistory/index.tsx`

**Interfaces:**
- Consumes: `api.market.movers`, `MarketItemHistory.trades/book`, `writeSelection`, strings from Task 4.

- [ ] **Step 1: Movers tab** `Tabs/Movers/index.tsx`:

```tsx
import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Group, NumberInput, SegmentedControl, SimpleGrid, Table, Text, Title } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { TauriTypes } from "$types";
import { writeSelection } from "../../selection";

function MoverTable({ title, rows, t, onOpen }: { title: string; rows: TauriTypes.MarketMover[]; t: (k: string) => string; onOpen: (m: TauriTypes.MarketMover) => void }) {
  return (
    <div>
      <Title order={5} mb="xs">{title}</Title>
      {rows.length === 0 ? (
        <Text c="dimmed">{t("empty")}</Text>
      ) : (
        <Table striped withTableBorder highlightOnHover>
          <Table.Thead>
            <Table.Tr>
              <Table.Th>{t("columns.item")}</Table.Th>
              <Table.Th>{t("columns.sub_type")}</Table.Th>
              <Table.Th>{t("columns.median_then")}</Table.Th>
              <Table.Th>{t("columns.median_now")}</Table.Th>
              <Table.Th>{t("columns.change_pct")}</Table.Th>
              <Table.Th>{t("columns.volume")}</Table.Th>
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {rows.map((m) => (
              <Table.Tr key={`${m.item_id}|${m.sub_type}`} style={{ cursor: "pointer" }} onClick={() => onOpen(m)}>
                <Table.Td>{m.name}</Table.Td>
                <Table.Td>{m.sub_type || "—"}</Table.Td>
                <Table.Td>{m.median_then.toFixed(1)}</Table.Td>
                <Table.Td>{m.median_now.toFixed(1)}</Table.Td>
                <Table.Td><Text c={m.change_pct < 0 ? "red" : "green"}>{m.change_pct.toFixed(1)}%</Text></Table.Td>
                <Table.Td>{m.volume.toFixed(2)}</Table.Td>
              </Table.Tr>
            ))}
          </Table.Tbody>
        </Table>
      )}
    </div>
  );
}

export function MoversPanel({ isActive, onOpenItem }: { isActive?: boolean; onOpenItem: () => void }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`market_data.tabs.movers.${key}`, context);
  const [period, setPeriod] = useState<"day" | "week">("day");
  const [minVolume, setMinVolume] = useState<number>(3);
  const { data } = useQuery({ queryKey: ["market_movers", minVolume], queryFn: () => api.market.movers(minVolume), enabled: !!isActive, refetchInterval: 60_000 });
  const list = data?.[period] ?? { up: [], down: [] };
  const open = (m: TauriTypes.MarketMover) => { writeSelection({ slug: m.slug, sub_type: m.sub_type }); onOpenItem(); };

  return (
    <>
      <Group mt="md" align="end">
        <SegmentedControl value={period} onChange={(v) => setPeriod(v as "day" | "week")} data={[{ value: "day", label: t("period.day") }, { value: "week", label: t("period.week") }]} />
        <NumberInput label={t("min_volume")} value={minVolume} onChange={(v) => setMinVolume(Number(v) || 0)} min={0} step={0.5} w={160} />
      </Group>
      <SimpleGrid cols={{ base: 1, md: 2 }} mt="md">
        <MoverTable title={t("up")} rows={list.up} t={t} onOpen={open} />
        <MoverTable title={t("down")} rows={list.down} t={t} onOpen={open} />
      </SimpleGrid>
    </>
  );
}
```

- [ ] **Step 2: Wire it.** `Tabs/index.ts` adds `export * from "./Movers";`. In `pages/market_data/index.tsx` insert between overview and warmup:

```tsx
      { id: "movers", label: useTranslateTabs("movers.title"), component: (isActive: boolean) => <MoversPanel isActive={isActive} onOpenItem={() => setActiveTab("price_history")} /> },
```

- [ ] **Step 3: Trades and book on Price History.** In `Tabs/PriceHistory/index.tsx`, after the daily chart `Paper` and still inside the `{data && (<>...</>)}` block:

```tsx
          <SimpleGrid cols={{ base: 1, md: 2 }}>
            <Paper withBorder p="sm">
              <Text fw={600} mb="xs">{t("book_title")}</Text>
              {!data.book ? (
                <Text c="dimmed">{t("no_book")}</Text>
              ) : (
                <>
                  <Text size="xs" c="dimmed">{t("last_swept", { at: dayjs(data.book.swept_at).format("YYYY-MM-DD HH:mm:ss") })}</Text>
                  <SimpleGrid cols={2}>
                    {(["sells", "buys"] as const).map((side) => (
                      <div key={side}>
                        <Text fw={500}>{t(side)}</Text>
                        {(side === "sells" ? data.book!.top_sells : data.book!.top_buys).map(([platinum, quantity], i) => (
                          <Text key={i} size="sm">{platinum} p × {quantity}</Text>
                        ))}
                      </div>
                    ))}
                  </SimpleGrid>
                </>
              )}
            </Paper>
            <Paper withBorder p="sm">
              <Text fw={600} mb="xs">{t("trades_title")}</Text>
              {data.trades.length === 0 ? (
                <Text c="dimmed">{t("no_trades")}</Text>
              ) : (
                <Table striped withTableBorder>
                  <Table.Thead>
                    <Table.Tr>
                      <Table.Th>{t("vanished_at")}</Table.Th>
                      <Table.Th>{t("side")}</Table.Th>
                      <Table.Th>{t("platinum")}</Table.Th>
                      <Table.Th>{t("quantity")}</Table.Th>
                    </Table.Tr>
                  </Table.Thead>
                  <Table.Tbody>
                    {data.trades.map((tr, i) => (
                      <Table.Tr key={i}>
                        <Table.Td>{dayjs(tr.vanished_at).format("MM-DD HH:mm")}</Table.Td>
                        <Table.Td>{tr.side}</Table.Td>
                        <Table.Td>{tr.platinum}</Table.Td>
                        <Table.Td>{tr.quantity}</Table.Td>
                      </Table.Tr>
                    ))}
                  </Table.Tbody>
                </Table>
              )}
            </Paper>
          </SimpleGrid>
```

Add `Table` to the `@mantine/core` import.

- [ ] **Step 4: Build**

Run: `python3 scripts/check-rpc-commands.py && (cd web && pnpm build)`
Expected: server and web counts equal, `0 missing`; tsc and vite clean, no new warnings.

- [ ] **Step 5: Commit and push**

```bash
git add web/src/pages/market_data
git commit -m "feat(web): add the Movers tab and recent trades plus the order book on Price History

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 6: Deploy and record acceptance (needs the user)

**Files:**
- Create: `docs/PHASE-6B-ACCEPTANCE.md`

- [ ] **Step 1: Ask the user for the go-ahead to deploy** (always required for ockohome). Show the commit and the rsync dry-run deletion list. Same procedure as `docs/PHASE-4D-ACCEPTANCE.md`.

- [ ] **Step 2: Run the M8 checks with the user in the browser:**

| # | Check | How |
|---|---|---|
| 1 | Overview row count ≈ `item_stats` rows; sorting by spread puts the widest first; a row click lands on Price History with that item selected | Compare the "of N items" text with Collector's item counts; click the top row |
| 2 | Movers show plausible items; one item's change matches its Price History daily chart | Click a riser, read the last two daily medians |
| 3 | Warm-up shows the current warm count and a projection; before 2026-09-22 warm is 0 with a non-zero bar on the 22nd or later | Read the cards and the chart |
| 4 | Price History for a busy item shows trades inside the daily min/max and a book whose top sell is at or above the hourly chart's latest min-sell | Pick a high-volume item from Overview |

- [ ] **Step 3: Write `docs/PHASE-6B-ACCEPTANCE.md`** in the shape of `docs/PHASE-4D-ACCEPTANCE.md`: header, local gate numbers, deploy evidence, the table with results, rulings, follow-ups.

- [ ] **Step 4: Commit and push**

```bash
git add docs/PHASE-6B-ACCEPTANCE.md
git commit -m "docs: record phase 6b market data acceptance

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

The merge into `main` is the controller's, after the gate is green on the branch tip.

---

## Self-review

- **Spec coverage.** M1 → Tasks 1, 3, 4 (overview rows, names via cache, browser sort/filter, row click hand-off); M2 → Tasks 2, 3, 5 (latest median vs 1 and 7 days back, min volume, 25 per side, two tables with a period control); M3 → Tasks 1, 3, 4 (counts, 7-day projection from tomorrow, both histograms, cards and charts); M4 → Tasks 3, 5 (50 newest trades, latest book, rendered under the charts); M5 → Tasks 3–5 (module names, tab order and ids, selection key, string keys); M6 → nothing built; M7 → Task 1 (overview skips missing, warm-up projection second day and never for 4 trades — the `soon` and `thin` items), Task 2 (movers with a thin item excluded), Task 3 (history 50 newest + book; rpc routable, non-numeric `min_volume` rejected), Tasks 4–5 (script + build); M8 → Task 6.
- **Placeholders.** None; conditional instructions name their exact alternative (closure ownership of the cache handle, helper expectations to confirm from `store.rs` tests).
- **Type consistency.** `OverviewRow`, `Mover`/`MoverList`/`Movers`, `Projection`/`HistogramBucket`/`Warmup`, `TradePoint`/`BookSnapshot` match the TypeScript interfaces field for field; `market_movers { min_volume: f64 }` is sent as `minVolume` (the dispatcher's `rename_all = "camelCase"`); `writeSelection`/`readSelection` share one key and one shape; `onOpenItem` is the same prop on Overview and Movers.
