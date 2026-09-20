# Phase 6d: Closed-Trade Price Source — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task with Opus implementers and a reviewer between tasks. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the trader a second, switchable price basis built from warframe.market's closed-trade daily statistics (the basis the desktop Quantframe app uses), blended with the collector's live data, plus a tab that compares both so the user can decide when to switch.

**Architecture:** A slow supervised loop refreshes every item's 90-day closed series once a day into a new table through the limiter's Cold lane. A pure `aggregate` turns the last seven days into per-item closed stats; a pure `blend` merges them with the collector's inferred `item_stats` according to a setting, and one shared loader (`effective_stats`) feeds the trader, the hot set, the Warm badge and the Warm-up tab. Default mode is `inferred`, so deploying changes no trading behaviour.

**Tech Stack:** Rust (tokio, reqwest, serde, chrono, sea-orm raw SQL over SQLite), React 19 + Mantine 9.4.1 + TanStack Query 5 + mantine-datatable, pnpm 11.3.0.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md` — read **§25 (P1–P11)** first; §24 K1–K3 for the statistics endpoint, mapping and retry rule; §5.5 and §16 C2–C3 for `item_stats`, `PriceSource` and the hot set. §25 takes precedence.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-phase-6d`, branch `phase-6d-closed-price-source` (from `main` at `0c62a6c`). Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target`.
- **Tests:** `cargo test -p utils --lib`, `cargo test -p qf_core --lib`, `cargo test -p qf-server`; never bare `--workspace`. Web: `python3 scripts/check-rpc-commands.py` and `(cd web && pnpm build)`. Output pristine (no new warnings).
- **Exact values (P1–P8):** tables `closed_stats_daily` and `closed_fetch_state`; outcomes `ok | missing | failed`; `CLOSED_PACE_S = 10`; idle sleep 60 s; cutoff = most recent 00:30 UTC; failed retry after 1 h; limiter lane `Cold`; retention 90 days; window = the 7 UTC days `today − 7 ..= today − 1`; `volume = Σ volume ÷ 7`; warm = `days ≥ 5 AND trades ≥ 10`; fresh = outcome `ok` and `fetched_at` within 3 days; guard needs inferred `volume × 7 ≥ 20`; settings `live_scraper.general.price_source` (`inferred` | `closed`, default `inferred`) and `live_scraper.general.fast_drop_guard_pct` (default `10`, disabled at `-1`); operation tag `FastDropGuard`; RPC `market_price_sources {}`; supervised task name `Collector:ClosedStats`; log component `ClosedStats`; strings under `pages.market_data.tabs.price_source.*`.
- **Never** change `item_stats_daily`, `insert_missing`'s `INSERT OR IGNORE`, or what the charts and Movers read (K2 stands). **Never** add fields to `collector::stats::ItemStats` (ten struct literals depend on its shape; the extras travel in `Effective`).
- **Default mode is `inferred`.** With it, every trader input must be byte-for-byte what it is on `main`; Task 3's identity test guards this.
- **`en.json`** by targeted text insertion only; confirm it parses and the diff is contiguous blocks.
- **Commits:** conventional commits ending with exactly `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`; `git push -u origin phase-6d-closed-price-source` after each task. Never touch `main`.
- Tasks 1–4 do not touch ockohome; Task 5 is deployed by the user with `!` commands and checked in the browser.

## File Structure (end of phase 6d)

```
crates/migration/src/m20260921_000001_create_closed_stats.rs   NEW  two tables + index
crates/migration/src/lib.rs                                    MOD  register the migration
crates/qf_core/src/collector/closed.rs                         NEW  upsert, fetch state, stale list, aggregate, load_fresh, refresh loop, status
crates/qf_core/src/collector/mod.rs                            MOD  pub mod closed;
crates/qf_core/src/collector/backfill.rs                       MOD  ClosedDay gains avg_price/wa_price; lane parameter; run also fills closed_stats_daily
crates/qf_core/src/collector/maintenance.rs                    MOD  90-day retention for closed_stats_daily
crates/qf_core/src/collector/runner.rs                         MOD  supervise Collector:ClosedStats
crates/qf_core/tests/fixtures/statistics_small.json            MOD  avg_price/wa_price on the last row
crates/qf_core/src/enums/price_source_mode.rs                  NEW  PriceSourceMode
crates/qf_core/src/enums/mod.rs                                MOD  export it
crates/qf_core/src/app/types/settings/live_scraper_general_settings.rs  MOD  two fields
crates/qf_core/src/trader/blend.rs                             NEW  Effective, blend
crates/qf_core/src/trader/compare.rs                           NEW  PriceSourceRow, CandidateCounts, compare
crates/qf_core/src/trader/mod.rs                               MOD  pub mod blend; pub mod compare;
crates/qf_core/src/trader/price_source.rs                      MOD  effective_stats, source_settings, from_effective, shift filter, guarded
crates/qf_core/src/trader/item.rs                              MOD  FastDropGuard tag on buy and sell paths
crates/qf_core/src/commands/market.rs                          MOD  overview/warmup via effective_stats; market_price_sources
crates/qf_core/src/commands/rpc.rs                             MOD  one row + test
web/src/types/tauri.type.ts                                    MOD  settings fields, MarketPriceSources types
web/src/api/market/index.ts                                    MOD  priceSources()
web/src/pages/market_data/Tabs/PriceSource/index.tsx           NEW  the tab
web/src/pages/market_data/Tabs/index.ts                        MOD  export
web/src/pages/market_data/index.tsx                            MOD  tab entry
web/src/components/Forms/Settings/Tabs/LiveTrading/Tabs/General/index.tsx  MOD  select + number input
web/public/lang/en.json                                        MOD  two blocks
docs/GO-LIVE-RUNBOOK.md                                        MOD  (Task 5) price-source pre-flight + rollback
docs/PHASE-6D-ACCEPTANCE.md                                    NEW  (Task 5)
```

---

### Task 1: Tables, `ClosedDay` extension, and the pure closed stats

**Files:**
- Create: `crates/migration/src/m20260921_000001_create_closed_stats.rs`, `crates/qf_core/src/collector/closed.rs`
- Modify: `crates/migration/src/lib.rs`, `crates/qf_core/src/collector/mod.rs` (add `pub mod closed;`), `crates/qf_core/src/collector/backfill.rs` (`ClosedDay`, `Row`, `parse_statistics`, its test literals), `crates/qf_core/tests/fixtures/statistics_small.json`

**Interfaces:**
- Consumes: `super::backfill::ClosedDay`, `super::store::{exec, tests::setup}`, `super::{db_err, stmt, ts}`.
- Produces:

```rust
// backfill.rs — two new fields, both Option<f64>, filled as is from the body
pub struct ClosedDay { pub sub_type: String, pub day: String, pub volume: i64, pub median: Option<f64>, pub min_price: Option<i64>, pub max_price: Option<i64>, pub avg_price: Option<f64>, pub wa_price: Option<f64> }

// closed.rs
pub const WINDOW_DAYS: i64 = 7;
pub const WARM_MIN_DAYS: usize = 5;
pub const FRESH_DAYS: i64 = 3;
#[derive(Debug, Clone, PartialEq)]
pub struct ClosedRow { pub item_id: String, pub sub_type: String, pub day: NaiveDate, pub volume: i64, pub median: Option<f64>, pub min_price: Option<i64>, pub max_price: Option<i64>, pub wa_price: Option<f64> }
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClosedStats { pub item_id: String, pub sub_type: String, pub volume: f64, pub moving_avg: Option<f64>, pub median: Option<f64>, pub avg_price: Option<f64>, pub min_price: Option<i64>, pub max_price: Option<i64>, pub week_price_shift: Option<f64>, pub days: usize, pub trades: i64, pub warm: bool }
pub async fn upsert_days(conn: &DatabaseConnection, item_id: &str, days: &[ClosedDay]) -> Result<u64, Error>;
pub async fn set_fetch_state(conn: &DatabaseConnection, item_id: &str, at: DateTime<Utc>, outcome: &str) -> Result<(), Error>;
pub fn aggregate(rows: Vec<ClosedRow>, today: NaiveDate, warm_min_trades: usize) -> Vec<ClosedStats>;
pub async fn load_fresh(conn: &DatabaseConnection, now: DateTime<Utc>, warm_min_trades: usize) -> Result<Vec<ClosedStats>, Error>;
```

- [ ] **Step 1: Migration.** Create `crates/migration/src/m20260921_000001_create_closed_stats.rs`, copying the shape of `m20260920_000001_add_helper_events_alerted_at.rs` (same `run` helper, same `MigrationTrait` impl) with:

```rust
// warframe.market closed-trade dailies and the per-item refresh state (spec §25 P1).
const UP: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS closed_stats_daily (
        item_id TEXT NOT NULL,
        sub_type TEXT NOT NULL,
        day TEXT NOT NULL,
        volume INTEGER NOT NULL,
        median REAL,
        min_price INTEGER,
        max_price INTEGER,
        avg_price REAL,
        wa_price REAL,
        PRIMARY KEY (item_id, sub_type, day)
    )",
    "CREATE INDEX IF NOT EXISTS idx_closed_stats_day ON closed_stats_daily (day)",
    "CREATE TABLE IF NOT EXISTS closed_fetch_state (
        item_id TEXT PRIMARY KEY,
        fetched_at TEXT NOT NULL,
        outcome TEXT NOT NULL
    )",
];
const DOWN: &[&str] = &["DROP TABLE IF EXISTS closed_fetch_state", "DROP TABLE IF EXISTS closed_stats_daily"];
```

Register it in `crates/migration/src/lib.rs`: add `mod m20260921_000001_create_closed_stats;` beside the other `mod` lines and `Box::new(m20260921_000001_create_closed_stats::Migration),` after the `m20260920_000001_add_helper_events_alerted_at` entry.

- [ ] **Step 2: Fixture.** In `crates/qf_core/tests/fixtures/statistics_small.json` change only the last `90days` row to:

```json
        {"datetime": "2026-09-15T00:00:00.000+00:00", "volume": 43, "min_price": 66.0, "max_price": 70.0, "median": 69.0, "avg_price": 68.0, "wa_price": 68.5}
```

- [ ] **Step 3: Extend `ClosedDay` (failing test first).** In `backfill.rs`'s test `parses_the_90_day_series_with_the_collector_sub_type_keys`, add `avg_price: None, wa_price: None` to the first three literals and `avg_price: Some(68.0), wa_price: Some(68.5)` to the fourth. Run `cargo test -p qf_core --lib collector::backfill` — expected: compile error, `ClosedDay` has no field `avg_price`. Then add `pub avg_price: Option<f64>, pub wa_price: Option<f64>` to `ClosedDay`, `avg_price: Option<f64>, wa_price: Option<f64>` to the private `Row`, and `avg_price: r.avg_price, wa_price: r.wa_price,` to the mapping in `parse_statistics`. Re-run — expected: PASS (all backfill tests).

- [ ] **Step 4: Write `closed.rs` tests first.** Create `crates/qf_core/src/collector/closed.rs` with the module doc `//! warframe.market closed-trade dailies as a price basis (spec §25 P1–P3).`, the imports below and this test module; leave the functions unwritten so it fails to compile:

```rust
use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use service::sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};
use utils::Error;

use super::backfill::ClosedDay;
use super::{db_err, stmt, ts};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;
    use crate::collector::store::tests::setup;

    fn day(d: &str) -> NaiveDate {
        NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()
    }
    fn row(d: &str, volume: i64, median: f64, wa: f64) -> ClosedRow {
        ClosedRow { item_id: "item1".into(), sub_type: String::new(), day: day(d), volume, median: Some(median), min_price: Some(median as i64 - 5), max_price: Some(median as i64 + 5), wa_price: Some(wa) }
    }
    fn closed_day(d: &str, volume: i64, median: f64) -> ClosedDay {
        ClosedDay { sub_type: String::new(), day: d.into(), volume, median: Some(median), min_price: Some(1), max_price: Some(99), avg_price: Some(median), wa_price: Some(median) }
    }

    #[test]
    fn aggregate_covers_the_seven_days_before_today_only() {
        // today = 2026-09-20, so W = 09-13 ..= 09-19. 09-12 and 09-20 are outside.
        let rows = vec![
            row("2026-09-12", 100, 500.0, 500.0),
            row("2026-09-13", 10, 60.0, 61.0),
            row("2026-09-15", 20, 64.0, 65.0),
            row("2026-09-16", 5, 66.0, 66.0),
            row("2026-09-18", 30, 70.0, 71.0),
            row("2026-09-19", 10, 68.0, 67.0),
            row("2026-09-20", 100, 500.0, 500.0),
        ];
        let stats = aggregate(rows, day("2026-09-20"), 10);
        assert_eq!(stats.len(), 1);
        let s = &stats[0];
        assert_eq!((s.days, s.trades), (5, 75));
        assert!((s.volume - 75.0 / 7.0).abs() < 1e-9, "trades per day over the whole week, empty days included");
        assert_eq!(s.moving_avg, Some((60.0 + 64.0 + 66.0 + 70.0 + 68.0) / 5.0));
        assert_eq!(s.median, Some(66.0));
        // two most recent days with volume: 09-18 (30 @ 71) and 09-19 (10 @ 67)
        assert_eq!(s.avg_price, Some((30.0 * 71.0 + 10.0 * 67.0) / 40.0));
        assert_eq!((s.min_price, s.max_price), (Some(55), Some(75)));
        assert_eq!(s.week_price_shift, Some(68.0 - 60.0));
        assert!(s.warm);
    }

    #[test]
    fn aggregate_warm_needs_five_days_and_ten_trades_and_shift_needs_two_days() {
        let four_days = vec![row("2026-09-16", 50, 10.0, 10.0), row("2026-09-17", 50, 10.0, 10.0), row("2026-09-18", 50, 10.0, 10.0), row("2026-09-19", 50, 10.0, 10.0)];
        assert!(!aggregate(four_days, day("2026-09-20"), 10)[0].warm, "four days is not enough");
        let thin = (13..=19).map(|d| row(&format!("2026-09-{d}"), 1, 10.0, 10.0)).collect();
        assert!(!aggregate(thin, day("2026-09-20"), 10)[0].warm, "seven trades is not enough");
        let one = aggregate(vec![row("2026-09-19", 3, 10.0, 10.0)], day("2026-09-20"), 10);
        assert_eq!(one[0].week_price_shift, None);
        assert!(aggregate(vec![row("2026-09-01", 3, 10.0, 10.0)], day("2026-09-20"), 10).is_empty(), "no row in W, no stats");
    }

    #[tokio::test]
    async fn upsert_replaces_an_existing_day() {
        let (_dir, conn) = setup().await;
        assert_eq!(upsert_days(&conn, "item1", &[closed_day("2026-09-19", 5, 40.0)]).await.unwrap(), 1);
        upsert_days(&conn, "item1", &[closed_day("2026-09-19", 9, 44.0)]).await.unwrap();
        let kept = conn.query_one(stmt("SELECT volume, median FROM closed_stats_daily WHERE item_id = 'item1' AND day = '2026-09-19'", vec![])).await.unwrap().unwrap();
        assert_eq!(kept.try_get::<i64>("", "volume").unwrap(), 9);
        assert_eq!(kept.try_get::<f64>("", "median").unwrap(), 44.0);
    }

    #[tokio::test]
    async fn load_fresh_needs_an_ok_state_within_three_days() {
        let (_dir, conn) = setup().await;
        let now = parse_ts("2026-09-20T08:00:00Z").unwrap();
        let week: Vec<ClosedDay> = (13..=19).map(|d| closed_day(&format!("2026-09-{d}"), 4, 50.0)).collect();
        upsert_days(&conn, "item1", &week).await.unwrap();
        upsert_days(&conn, "item2", &week).await.unwrap();
        assert!(load_fresh(&conn, now, 10).await.unwrap().is_empty(), "no fetch state yet");
        set_fetch_state(&conn, "item1", now - Duration::days(1), "ok").await.unwrap();
        set_fetch_state(&conn, "item2", now - Duration::days(4), "ok").await.unwrap();
        let fresh = load_fresh(&conn, now, 10).await.unwrap();
        assert_eq!(fresh.iter().map(|s| s.item_id.as_str()).collect::<Vec<_>>(), vec!["item1"], "item2's fetch is four days old");
        assert!(fresh[0].warm);
        set_fetch_state(&conn, "item1", now, "failed").await.unwrap();
        assert!(load_fresh(&conn, now, 10).await.unwrap().is_empty(), "a failed state is not fresh");
    }
}
```

Add `pub mod closed;` to `collector/mod.rs`. Run `cargo test -p qf_core --lib collector::closed` — expected: FAIL to compile (`ClosedRow`, `aggregate`, `upsert_days`, `set_fetch_state`, `load_fresh` not found).

- [ ] **Step 5: Implement.** Above the test module:

```rust
pub const WINDOW_DAYS: i64 = 7;
pub const WARM_MIN_DAYS: usize = 5;
pub const FRESH_DAYS: i64 = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct ClosedRow {
    pub item_id: String,
    pub sub_type: String,
    pub day: NaiveDate,
    pub volume: i64,
    pub median: Option<f64>,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
    pub wa_price: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClosedStats {
    pub item_id: String,
    pub sub_type: String,
    pub volume: f64,
    pub moving_avg: Option<f64>,
    pub median: Option<f64>,
    pub avg_price: Option<f64>,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
    pub week_price_shift: Option<f64>,
    pub days: usize,
    pub trades: i64,
    pub warm: bool,
}

fn median_f64(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let mid = sorted.len() / 2;
    Some(if sorted.len() % 2 == 0 { (sorted[mid - 1] + sorted[mid]) / 2.0 } else { sorted[mid] })
}

/// Closed stats over the seven UTC days before `today` (spec §25 P3). Rows outside that window are ignored.
pub fn aggregate(rows: Vec<ClosedRow>, today: NaiveDate, warm_min_trades: usize) -> Vec<ClosedStats> {
    let first = today - Duration::days(WINDOW_DAYS);
    let mut by_key: BTreeMap<(String, String), Vec<ClosedRow>> = BTreeMap::new();
    for r in rows.into_iter().filter(|r| r.day >= first && r.day < today) {
        by_key.entry((r.item_id.clone(), r.sub_type.clone())).or_default().push(r);
    }
    by_key
        .into_iter()
        .map(|((item_id, sub_type), mut days)| {
            days.sort_by_key(|r| r.day);
            let medians: Vec<f64> = days.iter().filter_map(|r| r.median).collect();
            let trades: i64 = days.iter().map(|r| r.volume).sum();
            let moving_avg = (!medians.is_empty()).then(|| medians.iter().sum::<f64>() / medians.len() as f64);
            let recent: Vec<&ClosedRow> = days.iter().rev().filter(|r| r.volume > 0 && r.wa_price.is_some()).take(2).collect();
            let recent_volume: i64 = recent.iter().map(|r| r.volume).sum();
            let avg_price = if recent_volume > 0 {
                Some(recent.iter().map(|r| r.wa_price.unwrap_or(0.0) * r.volume as f64).sum::<f64>() / recent_volume as f64)
            } else {
                moving_avg
            };
            let with_median: Vec<&ClosedRow> = days.iter().filter(|r| r.median.is_some()).collect();
            let week_price_shift = match (with_median.first(), with_median.last()) {
                (Some(a), Some(b)) if a.day != b.day => Some(b.median.unwrap_or(0.0) - a.median.unwrap_or(0.0)),
                _ => None,
            };
            ClosedStats {
                item_id,
                sub_type,
                volume: trades as f64 / WINDOW_DAYS as f64,
                moving_avg,
                median: median_f64(&medians),
                avg_price,
                min_price: days.iter().filter_map(|r| r.min_price).min(),
                max_price: days.iter().filter_map(|r| r.max_price).max(),
                week_price_shift,
                days: days.len(),
                trades,
                warm: days.len() >= WARM_MIN_DAYS && trades >= warm_min_trades as i64,
            }
        })
        .collect()
}

/// Upserts one item's days; this table has a single kind of writer (spec §25 P1).
pub async fn upsert_days(conn: &DatabaseConnection, item_id: &str, days: &[ClosedDay]) -> Result<u64, Error> {
    const C: &str = "ClosedStats:Upsert";
    let txn = conn.begin().await.map_err(|e| db_err(C, e))?;
    let mut written = 0;
    for d in days {
        written += txn
            .execute(stmt(
                "INSERT INTO closed_stats_daily (item_id, sub_type, day, volume, median, min_price, max_price, avg_price, wa_price)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (item_id, sub_type, day) DO UPDATE SET volume = excluded.volume, median = excluded.median,
                   min_price = excluded.min_price, max_price = excluded.max_price, avg_price = excluded.avg_price, wa_price = excluded.wa_price",
                vec![item_id.into(), d.sub_type.clone().into(), d.day.clone().into(), d.volume.into(), d.median.into(), d.min_price.into(), d.max_price.into(), d.avg_price.into(), d.wa_price.into()],
            ))
            .await
            .map_err(|e| db_err(C, e))?
            .rows_affected();
    }
    txn.commit().await.map_err(|e| db_err(C, e))?;
    Ok(written)
}

pub async fn set_fetch_state(conn: &DatabaseConnection, item_id: &str, at: DateTime<Utc>, outcome: &str) -> Result<(), Error> {
    conn.execute(stmt(
        "INSERT INTO closed_fetch_state (item_id, fetched_at, outcome) VALUES (?, ?, ?)
         ON CONFLICT (item_id) DO UPDATE SET fetched_at = excluded.fetched_at, outcome = excluded.outcome",
        vec![item_id.into(), ts(at).into(), outcome.into()],
    ))
    .await
    .map_err(|e| db_err("ClosedStats:State", e))?;
    Ok(())
}

/// Closed stats for every item whose last fetch was `ok` within `FRESH_DAYS` (spec §25 P3).
pub async fn load_fresh(conn: &DatabaseConnection, now: DateTime<Utc>, warm_min_trades: usize) -> Result<Vec<ClosedStats>, Error> {
    const C: &str = "ClosedStats:Load";
    let today = now.date_naive();
    let rows = conn
        .query_all(stmt(
            "SELECT d.item_id, d.sub_type, d.day, d.volume, d.median, d.min_price, d.max_price, d.wa_price
             FROM closed_stats_daily d JOIN closed_fetch_state f ON f.item_id = d.item_id
             WHERE f.outcome = 'ok' AND f.fetched_at >= ? AND d.day >= ? AND d.day < ?",
            vec![
                ts(now - Duration::days(FRESH_DAYS)).into(),
                (today - Duration::days(WINDOW_DAYS)).to_string().into(),
                today.to_string().into(),
            ],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            let day: String = r.try_get("", "day").map_err(|e| db_err(C, e))?;
            Ok(ClosedRow {
                item_id: r.try_get("", "item_id").map_err(|e| db_err(C, e))?,
                sub_type: r.try_get("", "sub_type").map_err(|e| db_err(C, e))?,
                day: NaiveDate::parse_from_str(&day, "%Y-%m-%d").map_err(|e| db_err(C, e))?,
                volume: r.try_get("", "volume").map_err(|e| db_err(C, e))?,
                median: r.try_get("", "median").map_err(|e| db_err(C, e))?,
                min_price: r.try_get("", "min_price").map_err(|e| db_err(C, e))?,
                max_price: r.try_get("", "max_price").map_err(|e| db_err(C, e))?,
                wa_price: r.try_get("", "wa_price").map_err(|e| db_err(C, e))?,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(aggregate(rows, today, warm_min_trades))
}
```

- [ ] **Step 6: Run.** `cargo test -p qf_core --lib collector::closed collector::backfill` — expected: PASS. Then `cargo test -p qf_core --lib` — expected: PASS, no new warnings.

- [ ] **Step 7: Commit.**

```bash
git add crates/migration crates/qf_core/src/collector crates/qf_core/tests/fixtures/statistics_small.json
git commit -m "feat(collector): store warframe.market closed-trade dailies and aggregate a seven-day window"
git push -u origin phase-6d-closed-price-source
```

---

### Task 2: The daily refresh loop, the import button's second table, and retention

**Files:**
- Modify: `crates/qf_core/src/collector/closed.rs` (stale list, `pick`, `refresh_once`, `refresh_status`, tests), `crates/qf_core/src/collector/backfill.rs` (`fetch_with_retries` lane parameter and `pub(crate)`; `run` fills the new table), `crates/qf_core/src/collector/maintenance.rs` (retention), `crates/qf_core/src/collector/runner.rs` (supervised loop)

**Interfaces:**
- Consumes (Task 1): `closed::{upsert_days, set_fetch_state}`, `backfill::{parse_statistics, StatisticsSource, HttpStatisticsSource}`, `fetch::FetchError`, `Limiter`, `Lane`.
- Produces:

```rust
// backfill.rs
pub(crate) async fn fetch_with_retries(source: &dyn StatisticsSource, limiter: &Limiter, lane: Lane, slug: &str) -> Result<String, FetchError>;

// closed.rs
pub const CLOSED_PACE_S: u64 = 10;
pub const IDLE_SLEEP_S: u64 = 60;
pub const CLOSED_RETENTION_DAYS: i64 = 90;
pub fn cutoff(now: DateTime<Utc>) -> DateTime<Utc>;                       // most recent 00:30 UTC
pub async fn stale_items(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<Vec<(String, String)>, Error>;  // (item_id, slug), never fetched first, then oldest
pub fn pick(stale: &[(String, String)], hot: &HashSet<String>) -> Option<(String, String)>;                       // first hot item, else the first
pub async fn refresh_once(conn: &DatabaseConnection, source: &dyn StatisticsSource, limiter: &Limiter, hot: &HashSet<String>, now: DateTime<Utc>) -> Result<Option<usize>, Error>;  // None = nothing stale; Some(n) = stale count before this fetch
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RefreshStatus { pub active: i64, pub ok: i64, pub missing: i64, pub failed: i64, pub stale: i64, pub oldest_fetched_at: Option<String> }
pub async fn refresh_status(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<RefreshStatus, Error>;
pub async fn apply_retention(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<u64, Error>;
```

- [ ] **Step 1: Lane parameter.** In `backfill.rs` change `fetch_with_retries` to `pub(crate)`, add the `lane: Lane` parameter after `limiter`, replace `limiter.acquire(Lane::Hot)` with `limiter.acquire(lane)`, and pass `Lane::Hot` at its one call site in `run`. Run `cargo test -p qf_core --lib collector::backfill` — expected: PASS (behaviour unchanged).

- [ ] **Step 2: Failing tests.** Add `use std::collections::HashSet;`, `use super::backfill::{fetch_with_retries, parse_statistics, StatisticsSource};`, `use super::fetch::FetchError;`, `use super::store::exec;`, `use crate::market::limiter::{Lane, Limiter};`, `use utils::{get_location, warning, LoggerOptions};` to `closed.rs`, and these tests to its test module:

```rust
    use crate::collector::backfill::StatsFuture;

    const SMALL: &str = include_str!("../../tests/fixtures/statistics_small.json");

    struct Scripted(std::sync::Mutex<std::collections::HashMap<String, std::collections::VecDeque<Result<String, FetchError>>>>);
    impl StatisticsSource for Scripted {
        fn fetch<'a>(&'a self, slug: &'a str) -> StatsFuture<'a> {
            let next = self.0.lock().unwrap().get_mut(slug).and_then(|q| q.pop_front()).unwrap_or(Err(FetchError::Transient("unscripted".into())));
            Box::pin(async move { next })
        }
    }
    fn scripted(entries: Vec<(&str, Vec<Result<String, FetchError>>)>) -> Scripted {
        Scripted(std::sync::Mutex::new(entries.into_iter().map(|(k, v)| (k.to_string(), v.into())).collect()))
    }

    #[test]
    fn cutoff_is_the_most_recent_half_past_midnight_utc() {
        assert_eq!(ts(cutoff(parse_ts("2026-09-20T08:00:00Z").unwrap())), "2026-09-20T00:30:00Z");
        assert_eq!(ts(cutoff(parse_ts("2026-09-20T00:10:00Z").unwrap())), "2026-09-19T00:30:00Z");
    }

    #[tokio::test]
    async fn stale_items_honour_the_cutoff_the_failed_hour_and_inactive_items() {
        let (_dir, conn) = setup().await; // item1/slug1 and item2/slug2, both active
        let now = parse_ts("2026-09-20T08:00:00Z").unwrap();
        assert_eq!(stale_items(&conn, now).await.unwrap().len(), 2, "never fetched");
        set_fetch_state(&conn, "item1", now - Duration::hours(2), "ok").await.unwrap();
        assert_eq!(stale_items(&conn, now).await.unwrap(), vec![("item2".to_string(), "slug2".to_string())], "item1 was fetched after today's cutoff");
        set_fetch_state(&conn, "item1", now - Duration::hours(9), "ok").await.unwrap();
        assert_eq!(stale_items(&conn, now).await.unwrap()[0].0, "item2", "never fetched sorts before an old fetch");
        set_fetch_state(&conn, "item2", now - Duration::minutes(30), "failed").await.unwrap();
        assert_eq!(stale_items(&conn, now).await.unwrap().len(), 1, "a failure is retried only after an hour");
        set_fetch_state(&conn, "item2", now - Duration::minutes(90), "failed").await.unwrap();
        assert_eq!(stale_items(&conn, now).await.unwrap().len(), 2);
        exec(&conn, "Test", "UPDATE sweep_state SET active = 0 WHERE item_id = 'item1'", vec![]).await.unwrap();
        assert_eq!(stale_items(&conn, now).await.unwrap().len(), 1, "inactive items are skipped");
    }

    #[test]
    fn pick_prefers_a_hot_item() {
        let stale = vec![("a".to_string(), "sa".to_string()), ("b".to_string(), "sb".to_string())];
        assert_eq!(pick(&stale, &HashSet::new()).unwrap().0, "a");
        assert_eq!(pick(&stale, &HashSet::from(["b".to_string()])).unwrap().0, "b");
        assert!(pick(&[], &HashSet::new()).is_none());
    }

    #[tokio::test]
    async fn refresh_once_writes_ok_missing_and_failed_states() {
        let (_dir, conn) = setup().await;
        let now = parse_ts("2026-09-20T08:00:00Z").unwrap();
        let limiter = Limiter::new(1000);
        let source = scripted(vec![
            ("slug1", vec![Ok(SMALL.to_string())]),
            ("slug2", vec![Err(FetchError::NotFound)]),
        ]);
        assert_eq!(refresh_once(&conn, &source, &limiter, &HashSet::new(), now).await.unwrap(), Some(2));
        assert_eq!(refresh_once(&conn, &source, &limiter, &HashSet::new(), now).await.unwrap(), Some(1));
        assert_eq!(refresh_once(&conn, &source, &limiter, &HashSet::new(), now).await.unwrap(), None, "both were fetched after the cutoff");
        let status = refresh_status(&conn, now).await.unwrap();
        assert_eq!((status.active, status.ok, status.missing, status.failed, status.stale), (2, 1, 1, 0, 0));
        let days: i64 = conn.query_one(stmt("SELECT COUNT(*) AS n FROM closed_stats_daily WHERE item_id = 'item1'", vec![])).await.unwrap().unwrap().try_get("", "n").unwrap();
        assert_eq!(days, 4, "the four fixture rows");

        let later = now + Duration::days(1);
        let dead = scripted(vec![("slug1", vec![Err(FetchError::Transient("1".into())), Err(FetchError::Transient("2".into())), Err(FetchError::Transient("3".into()))])]);
        refresh_once(&conn, &dead, &limiter, &HashSet::from(["item1".to_string()]), later).await.unwrap();
        assert_eq!(refresh_status(&conn, later).await.unwrap().failed, 1);
    }

    #[tokio::test]
    async fn retention_drops_days_older_than_ninety() {
        let (_dir, conn) = setup().await;
        upsert_days(&conn, "item1", &[closed_day("2026-06-01", 1, 1.0), closed_day("2026-09-19", 1, 1.0)]).await.unwrap();
        assert_eq!(apply_retention(&conn, parse_ts("2026-09-20T08:00:00Z").unwrap()).await.unwrap(), 1);
    }
```

Run `cargo test -p qf_core --lib collector::closed` — expected: FAIL to compile (`cutoff`, `stale_items`, `pick`, `refresh_once`, `refresh_status`, `apply_retention` not found).

- [ ] **Step 3: Implement** in `closed.rs`:

```rust
pub const CLOSED_PACE_S: u64 = 10;
pub const IDLE_SLEEP_S: u64 = 60;
pub const CLOSED_RETENTION_DAYS: i64 = 90;
const FAILED_RETRY: i64 = 1; // hours

/// warframe.market publishes yesterday's row shortly after midnight UTC; half past is a safe margin (spec §25 P2).
pub fn cutoff(now: DateTime<Utc>) -> DateTime<Utc> {
    let today = now.date_naive().and_hms_opt(0, 30, 0).expect("valid time").and_utc();
    if now >= today { today } else { today - Duration::days(1) }
}

/// Active items that need a fetch: never fetched first, then oldest fetch, then item id (spec §25 P2).
pub async fn stale_items(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<Vec<(String, String)>, Error> {
    const C: &str = "ClosedStats:Stale";
    conn.query_all(stmt(
        "SELECT s.item_id, s.slug FROM sweep_state s LEFT JOIN closed_fetch_state f ON f.item_id = s.item_id
         WHERE s.active = 1 AND (
               f.item_id IS NULL
            OR f.fetched_at < ?
            OR (f.outcome = 'failed' AND f.fetched_at < ?))
         ORDER BY f.fetched_at IS NOT NULL, f.fetched_at, s.item_id",
        vec![ts(cutoff(now)).into(), ts(now - Duration::hours(FAILED_RETRY)).into()],
    ))
    .await
    .map_err(|e| db_err(C, e))?
    .iter()
    .map(|r| Ok((r.try_get("", "item_id").map_err(|e| db_err(C, e))?, r.try_get("", "slug").map_err(|e| db_err(C, e))?)))
    .collect()
}

pub fn pick(stale: &[(String, String)], hot: &HashSet<String>) -> Option<(String, String)> {
    stale.iter().find(|(id, _)| hot.contains(id)).or_else(|| stale.first()).cloned()
}

/// Fetches one stale item through the Cold lane. `None` when nothing is stale; otherwise the stale count before this fetch.
pub async fn refresh_once(conn: &DatabaseConnection, source: &dyn StatisticsSource, limiter: &Limiter, hot: &HashSet<String>, now: DateTime<Utc>) -> Result<Option<usize>, Error> {
    const C: &str = "ClosedStats";
    let stale = stale_items(conn, now).await?;
    let Some((item_id, slug)) = pick(&stale, hot) else { return Ok(None) };
    let outcome = match fetch_with_retries(source, limiter, Lane::Cold, &slug).await {
        Ok(body) => match parse_statistics(&body) {
            Ok(days) => upsert_days(conn, &item_id, &days).await.map(|_| "ok"),
            Err(e) => Err(e),
        },
        Err(FetchError::NotFound) => Ok("missing"),
        Err(e) => Err(Error::new(C, e.to_string(), get_location!())),
    };
    let outcome = outcome.unwrap_or_else(|e| {
        warning(C, format!("{slug}: {}", e.message), &LoggerOptions::default());
        "failed"
    });
    set_fetch_state(conn, &item_id, now, outcome).await?;
    Ok(Some(stale.len()))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RefreshStatus {
    pub active: i64,
    pub ok: i64,
    pub missing: i64,
    pub failed: i64,
    pub stale: i64,
    pub oldest_fetched_at: Option<String>,
}

pub async fn refresh_status(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<RefreshStatus, Error> {
    const C: &str = "ClosedStats:Status";
    let row = conn
        .query_one(stmt(
            "SELECT COUNT(*) AS active,
                    COALESCE(SUM(f.outcome = 'ok'), 0) AS ok,
                    COALESCE(SUM(f.outcome = 'missing'), 0) AS missing,
                    COALESCE(SUM(f.outcome = 'failed'), 0) AS failed,
                    MIN(f.fetched_at) AS oldest
             FROM sweep_state s LEFT JOIN closed_fetch_state f ON f.item_id = s.item_id WHERE s.active = 1",
            vec![],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .ok_or_else(|| db_err(C, "no status row"))?;
    Ok(RefreshStatus {
        active: row.try_get("", "active").map_err(|e| db_err(C, e))?,
        ok: row.try_get("", "ok").map_err(|e| db_err(C, e))?,
        missing: row.try_get("", "missing").map_err(|e| db_err(C, e))?,
        failed: row.try_get("", "failed").map_err(|e| db_err(C, e))?,
        stale: stale_items(conn, now).await?.len() as i64,
        oldest_fetched_at: row.try_get("", "oldest").map_err(|e| db_err(C, e))?,
    })
}

pub async fn apply_retention(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<u64, Error> {
    exec(conn, "ClosedStats:Retention", "DELETE FROM closed_stats_daily WHERE day < ?", vec![(now.date_naive() - Duration::days(CLOSED_RETENTION_DAYS)).to_string().into()]).await
}
```

Run `cargo test -p qf_core --lib collector::closed` — expected: PASS.

- [ ] **Step 4: The import button fills the new table too (failing test first).** In `backfill.rs`'s test `run_counts_inserted_missing_and_failed_items_and_reports_progress`, append before the closing brace:

```rust
        let closed: i64 = conn.query_one(stmt("SELECT COUNT(*) AS n FROM closed_stats_daily", vec![])).await.unwrap().unwrap().try_get("", "n").unwrap();
        assert_eq!(closed, 8, "the run fills closed_stats_daily as well (spec §25 P2)");
        let states: i64 = conn.query_one(stmt("SELECT COUNT(*) AS n FROM closed_fetch_state WHERE outcome = 'ok'", vec![])).await.unwrap().unwrap().try_get("", "n").unwrap();
        assert_eq!(states, 2);
```

Run it — expected: FAIL (`closed` is 0). Then in `run`, replace the `Ok(days) => insert_missing(conn, &item_id, &days).await.map(Some),` arm with the block below. `run` returns `BackfillStatus`, not a `Result`, so the three fallible calls sit in an async block whose `Result` feeds the existing `outcome` match:

```rust
                Ok(days) => async {
                    super::closed::upsert_days(conn, &item_id, &days).await?;
                    super::closed::set_fetch_state(conn, &item_id, Utc::now(), "ok").await?;
                    insert_missing(conn, &item_id, &days).await.map(Some)
                }
                .await,
```

Re-run — expected: PASS, and `days_inserted` is still 8 (it counts `item_stats_daily` rows only, so the status shape and the K7 numbers do not change).

- [ ] **Step 5: Retention.** In `maintenance.rs`'s `hourly`, after the `apply_retention` line add `super::closed::apply_retention(conn, now).await?;`. `HourlyReport` and its log line stay as they are.

- [ ] **Step 6: Supervised loop.** In `runner.rs` add `use super::backfill::HttpStatisticsSource;` and `use super::closed;`, then:

```rust
const WFM_API_V1: &str = "https://api.warframe.market/v1";

/// One closed-statistics fetch every `CLOSED_PACE_S` while anything is stale (spec §25 P2).
async fn closed_stats_loop(collector: Arc<Collector>, http: reqwest::Client) {
    let source = HttpStatisticsSource::new(http, WFM_API_V1);
    let mut draining = false;
    loop {
        match closed::refresh_once(&collector.conn, &source, collector.limiter, &collector.hot_ids(), Utc::now()).await {
            Ok(Some(_)) => {
                draining = true;
                tokio::time::sleep(StdDuration::from_secs(closed::CLOSED_PACE_S)).await;
            }
            Ok(None) => {
                if draining {
                    draining = false;
                    match closed::refresh_status(&collector.conn, Utc::now()).await {
                        Ok(s) => info("ClosedStats", format!("Pass complete: ok {}, missing {}, failed {}", s.ok, s.missing, s.failed), &LoggerOptions::default()),
                        Err(e) => log_error("ClosedStats", &e),
                    }
                }
                tokio::time::sleep(StdDuration::from_secs(closed::IDLE_SLEEP_S)).await;
            }
            Err(e) => {
                log_error("ClosedStats", &e);
                tokio::time::sleep(ERROR_PAUSE).await;
            }
        }
    }
}
```

In `start`, the existing `item_refresh_loop` closure moves `http`; clone it first. Replace the `let (c, dir) = …` block with:

```rust
    let (c, dir, item_http) = (collector.clone(), opts.cache_dir, http.clone());
    supervise("Collector:ItemRefresh", RESTART_DELAY, move || item_refresh_loop(c.clone(), dir.clone(), item_http.clone()));
    let c = collector.clone();
    supervise("Collector:ClosedStats", RESTART_DELAY, move || closed_stats_loop(c.clone(), http.clone()));
```

- [ ] **Step 7: Run.** `cargo test -p qf_core --lib` and `cargo test -p qf-server` — expected: PASS, no new warnings.

- [ ] **Step 8: Commit.**

```bash
git add crates/qf_core/src/collector
git commit -m "feat(collector): refresh closed-trade statistics daily through the cold lane"
git push
```

---

### Task 3: Settings, the blend, and one shared loader

**Files:**
- Create: `crates/qf_core/src/enums/price_source_mode.rs`, `crates/qf_core/src/trader/blend.rs`
- Modify: `crates/qf_core/src/enums/mod.rs`, `crates/qf_core/src/app/types/settings/live_scraper_general_settings.rs`, `crates/qf_core/src/trader/mod.rs` (add `pub mod blend;`), `crates/qf_core/src/trader/price_source.rs`, `crates/qf_core/src/trader/item.rs`, `crates/qf_core/src/commands/market.rs`

**Interfaces:**
- Consumes (Task 1): `collector::closed::{ClosedStats, load_fresh}`; existing `collector::stats::{ItemStats, StatsConfig}`, `price_source::{all_item_stats, is_disabled}`.
- Produces:

```rust
// enums/price_source_mode.rs
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PriceSourceMode { #[default] Inferred, Closed }

// settings: LiveScraperGeneralSettings gains
pub price_source: PriceSourceMode,        // default Inferred
pub fast_drop_guard_pct: i64,             // default 10, disabled at -1

// trader/blend.rs
pub const GUARD_MIN_WEEK_TRADES: f64 = 20.0;
#[derive(Debug, Clone, PartialEq)]
pub struct Effective { pub stats: ItemStats, pub week_price_shift: Option<f64>, pub guarded: bool, pub closed: bool }
pub fn blend(inferred: Vec<ItemStats>, closed: Vec<ClosedStats>, mode: PriceSourceMode, guard_pct: i64, now: DateTime<Utc>) -> Vec<Effective>;

// trader/price_source.rs
pub struct ItemPriceInfo { /* existing fields */ pub guarded: bool }                      // #[serde(default)]
pub fn source_settings() -> (PriceSourceMode, i64);                                        // (Inferred, -1) when app state is missing
pub async fn effective_stats(conn: &DatabaseConnection, mode: PriceSourceMode, guard_pct: i64, now: DateTime<Utc>) -> Result<Vec<Effective>, Error>;
impl StatsPriceSource { pub fn from_effective(rows: Vec<Effective>, url_of: impl Fn(&str) -> Option<String>) -> Self; }   // from_stats stays and delegates
```

- [ ] **Step 1: The mode enum and the settings.** Create `enums/price_source_mode.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Which statistics feed the trader's price inputs (spec §25 P7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PriceSourceMode {
    /// The collector's probable-trade statistics (§5.5).
    #[default]
    Inferred,
    /// warframe.market closed-trade dailies blended with the collector's live data (§25).
    Closed,
}
```

Add to `enums/mod.rs`: `pub mod price_source_mode;` and `pub use price_source_mode::*;`. In `live_scraper_general_settings.rs` change the import to `use crate::enums::{PriceSourceMode, StockMode, TradeMode};`, add the fields at the end of the struct —

```rust
    #[serde(default)]
    pub price_source: PriceSourceMode,
    #[serde(default = "default_fast_drop_guard_pct")]
    pub fast_drop_guard_pct: i64,
```

— add `fn default_fast_drop_guard_pct() -> i64 { 10 }` below the struct, and `price_source: PriceSourceMode::Inferred, fast_drop_guard_pct: default_fast_drop_guard_pct(),` to `Default`. Add a test module at the bottom of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_saved_before_phase_6d_load_with_the_inferred_source_and_a_ten_percent_guard() {
        let old = r#"{"report_to_wfm":true,"auto_delete":false,"auto_trade":true,"stock_mode":"all","trade_modes":["buy"],"delete_conflicting_orders":false}"#;
        let s: LiveScraperGeneralSettings = serde_json::from_str(old).unwrap();
        assert_eq!((s.price_source, s.fast_drop_guard_pct), (PriceSourceMode::Inferred, 10));
        let json = serde_json::to_value(LiveScraperGeneralSettings { price_source: PriceSourceMode::Closed, ..Default::default() }).unwrap();
        assert_eq!(json["price_source"], "closed");
    }
}
```

Run `cargo test -p qf_core --lib settings::live_scraper_general_settings` — expected: PASS.

- [ ] **Step 2: Blend tests first.** Create `trader/blend.rs`:

```rust
//! Inferred and closed statistics merged into the trader's effective inputs (spec §25 P4–P5). Pure, no I/O.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::collector::closed::ClosedStats;
use crate::collector::stats::ItemStats;
use crate::collector::ts;
use crate::enums::PriceSourceMode;
use crate::trader::price_source::is_disabled;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;

    fn now() -> DateTime<Utc> {
        parse_ts("2026-09-20T08:00:00Z").unwrap()
    }
    fn inferred(id: &str, volume: f64, moving_avg: f64, avg_48h: f64, profit: f64) -> ItemStats {
        ItemStats { item_id: id.into(), sub_type: String::new(), volume, avg_price: Some(avg_48h), moving_avg: Some(moving_avg), profit: Some(profit), min_price: Some(1), max_price: Some(2), median: Some(moving_avg), history_days: 5, warm: false, updated_at: "2026-09-20T07:55:00Z".into() }
    }
    fn closed(id: &str, volume: f64, moving_avg: f64) -> ClosedStats {
        ClosedStats { item_id: id.into(), sub_type: String::new(), volume, moving_avg: Some(moving_avg), median: Some(moving_avg + 1.0), avg_price: Some(moving_avg + 2.0), min_price: Some(40), max_price: Some(90), week_price_shift: Some(-3.0), days: 7, trades: (volume * 7.0) as i64, warm: true }
    }

    #[test]
    fn inferred_mode_is_the_identity() {
        let rows = vec![inferred("a", 9.0, 70.0, 50.0, 12.0)];
        let out = blend(rows.clone(), vec![closed("a", 30.0, 66.0)], PriceSourceMode::Inferred, 10, now());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], Effective { stats: rows[0].clone(), week_price_shift: None, guarded: false, closed: false });
    }

    #[test]
    fn closed_mode_takes_the_closed_fields_and_keeps_the_inferred_profit() {
        let out = blend(vec![inferred("a", 9.0, 70.0, 69.0, 12.0)], vec![closed("a", 30.0, 66.0)], PriceSourceMode::Closed, 10, now());
        let e = &out[0];
        assert!(e.closed && !e.guarded);
        assert_eq!((e.stats.volume, e.stats.moving_avg, e.stats.median, e.stats.avg_price), (30.0, Some(66.0), Some(67.0), Some(68.0)));
        assert_eq!((e.stats.min_price, e.stats.max_price, e.stats.warm), (Some(40), Some(90), true));
        assert_eq!((e.stats.profit, e.stats.history_days), (Some(12.0), 5), "profit and history stay inferred");
        assert_eq!(e.week_price_shift, Some(-3.0));
    }

    #[test]
    fn closed_mode_falls_back_per_key_and_keeps_closed_only_keys() {
        let out = blend(vec![inferred("only_inferred", 9.0, 70.0, 69.0, 12.0)], vec![closed("only_closed", 30.0, 66.0)], PriceSourceMode::Closed, 10, now());
        assert_eq!(out.len(), 2);
        let c = out.iter().find(|e| e.stats.item_id == "only_closed").unwrap();
        assert_eq!((c.stats.profit, c.stats.history_days, c.closed), (None, 0, true));
        assert_eq!(c.stats.updated_at, ts(now()));
        let i = out.iter().find(|e| e.stats.item_id == "only_inferred").unwrap();
        assert_eq!((i.stats.moving_avg, i.closed, i.week_price_shift), (Some(70.0), false, None));
    }

    #[test]
    fn the_guard_lowers_closed_avg_only_past_the_percentage_with_enough_trades_and_when_enabled() {
        let run = |volume: f64, avg_48h: f64, pct: i64| blend(vec![inferred("a", volume, 99.0, avg_48h, 5.0)], vec![closed("a", 30.0, 100.0)], PriceSourceMode::Closed, pct, now()).remove(0);
        let fired = run(3.0, 85.0, 10);
        assert!(fired.guarded);
        assert_eq!(fired.stats.moving_avg, Some(85.0));
        assert!(!run(3.0, 95.0, 10).guarded, "5 % below is inside the 10 % band");
        assert!(!run(3.0, 90.0, 10).guarded, "exactly 10 % below does not fire");
        assert!(!run(2.0, 85.0, 10).guarded, "14 inferred trades a week is too thin");
        assert!(!run(3.0, 85.0, -1).guarded, "disabled");
        assert!(!run(3.0, 120.0, 10).guarded, "the guard never raises closed_avg");
    }
}
```

Add `pub mod blend;` to `trader/mod.rs`. Run `cargo test -p qf_core --lib trader::blend` — expected: FAIL to compile (`Effective`, `blend` not found).

- [ ] **Step 3: Implement `blend`** above the tests:

```rust
pub const GUARD_MIN_WEEK_TRADES: f64 = 20.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Effective {
    pub stats: ItemStats,
    pub week_price_shift: Option<f64>,
    pub guarded: bool,
    /// The price fields come from closed statistics.
    pub closed: bool,
}

/// `closed` must already be limited to fresh items (`closed::load_fresh`).
pub fn blend(inferred: Vec<ItemStats>, closed: Vec<ClosedStats>, mode: PriceSourceMode, guard_pct: i64, now: DateTime<Utc>) -> Vec<Effective> {
    let plain = |stats: ItemStats| Effective { stats, week_price_shift: None, guarded: false, closed: false };
    if mode == PriceSourceMode::Inferred {
        return inferred.into_iter().map(plain).collect();
    }
    let mut closed_by_key: BTreeMap<(String, String), ClosedStats> = closed.into_iter().map(|c| ((c.item_id.clone(), c.sub_type.clone()), c)).collect();
    let mut out: Vec<Effective> = inferred
        .into_iter()
        .map(|i| match closed_by_key.remove(&(i.item_id.clone(), i.sub_type.clone())) {
            Some(c) => merge(Some(i), c, guard_pct, now),
            None => plain(i),
        })
        .collect();
    out.extend(closed_by_key.into_values().map(|c| merge(None, c, guard_pct, now)));
    out
}

fn merge(inferred: Option<ItemStats>, c: ClosedStats, guard_pct: i64, now: DateTime<Utc>) -> Effective {
    // spec §25 P5: the collector sees a falling market within minutes; the closed average is a week old by construction.
    let fast_drop = inferred.as_ref().and_then(|i| {
        let (recent, closed_avg) = (i.avg_price?, c.moving_avg?);
        (!is_disabled(guard_pct) && i.volume * 7.0 >= GUARD_MIN_WEEK_TRADES && recent < closed_avg * (1.0 - guard_pct as f64 / 100.0)).then_some(recent)
    });
    Effective {
        stats: ItemStats {
            item_id: c.item_id,
            sub_type: c.sub_type,
            volume: c.volume,
            avg_price: c.avg_price,
            moving_avg: fast_drop.or(c.moving_avg),
            profit: inferred.as_ref().and_then(|i| i.profit),
            min_price: c.min_price,
            max_price: c.max_price,
            median: c.median,
            history_days: inferred.as_ref().map(|i| i.history_days).unwrap_or(0),
            warm: c.warm,
            updated_at: inferred.map(|i| i.updated_at).unwrap_or_else(|| ts(now)),
        },
        week_price_shift: c.week_price_shift,
        guarded: fast_drop.is_some(),
        closed: true,
    }
}
```

Run `cargo test -p qf_core --lib trader::blend` — expected: PASS.

- [ ] **Step 4: `price_source.rs` tests first.** Add to its test module:

```rust
    #[test]
    fn the_shift_filter_applies_only_to_items_that_have_a_shift() {
        use crate::trader::blend::Effective;
        let eff = |id: &str, shift: Option<f64>| Effective { stats: stats(id, "", 50.0, 50.0, 100.0), week_price_shift: shift, guarded: id == "falling", closed: shift.is_some() };
        let prices = StatsPriceSource::from_effective(vec![eff("rising", Some(4.0)), eff("falling", Some(-9.0)), eff("unknown", None)], |id| Some(format!("{id}_slug")));
        let mut settings = ItemSettings::default();
        settings.wtb.price_shift_threshold = -5;
        let mut ids: Vec<String> = get_interesting_items(&settings, &prices).into_iter().map(|i| i.wfm_id).collect();
        ids.sort();
        assert_eq!(ids, vec!["rising", "unknown"], "-9 is below the -5 threshold; no shift always passes");
        settings.wtb.price_shift_threshold = -1;
        assert_eq!(get_interesting_items(&settings, &prices).len(), 3, "disabled");
        assert!(prices.find_by("falling", &None).unwrap().guarded);
        assert_eq!(prices.find_by("rising", &None).unwrap().week_price_shift, 4.0);
    }
```

Note `-1` is the codebase's "disabled" value (`is_disabled`), so a threshold of exactly −1 p cannot be expressed; that matches the desktop app and is accepted. Run `cargo test -p qf_core --lib trader::price_source` — expected: FAIL to compile (`from_effective`, `guarded` not found).

- [ ] **Step 5: Implement in `price_source.rs`.**
  1. `ItemPriceInfo`: add `#[serde(default)] pub guarded: bool,` after `history_days`.
  2. Rename the body of `from_stats` into `from_effective(rows: Vec<Effective>, url_of: …)`: iterate `rows`, bind `let s = e.stats;`, set `week_price_shift: e.week_price_shift.unwrap_or(0.0)`, `guarded: e.guarded`, and keep a private `has_shift: HashSet<(String, String)>` on `StatsPriceSource` holding the keys whose `week_price_shift` was `Some` (add the field to the struct; `#[derive(Default)]` still works). Keep `from_stats` as:

```rust
    pub fn from_stats(stats: Vec<ItemStats>, url_of: impl Fn(&str) -> Option<String>) -> Self {
        Self::from_effective(stats.into_iter().map(|stats| Effective { stats, week_price_shift: None, guarded: false, closed: false }).collect(), url_of)
    }
```

  3. `PriceSource` trait: add `fn has_shift(&self, wfm_id: &str, sub_type_key: &str) -> bool { false }` with that default body, and implement it on `StatsPriceSource` as `self.has_shift.contains(&(wfm_id.to_string(), sub_type_key.to_string()))`. In `get_interesting_items` add, after the `avg_price_cap` filter:

```rust
        .filter(|i| is_disabled(wtb.price_shift_threshold) || !prices.has_shift(&i.wfm_id, &key_of(&i.sub_type)) || i.week_price_shift >= wtb.price_shift_threshold as f64)
```

  4. Loader and settings accessor:

```rust
/// `(mode, guard_pct)` from the live settings; inferred with the guard off when the app state is not up yet.
pub fn source_settings() -> (PriceSourceMode, i64) {
    states::try_app_state()
        .map(|app| (app.settings.live_scraper.general.price_source, app.settings.live_scraper.general.fast_drop_guard_pct))
        .unwrap_or((PriceSourceMode::Inferred, -1))
}

/// The one loader every consumer shares (spec §25 P7).
pub async fn effective_stats(conn: &DatabaseConnection, mode: PriceSourceMode, guard_pct: i64, now: DateTime<Utc>) -> Result<Vec<Effective>, Error> {
    let inferred = all_item_stats(conn).await?;
    let closed = match mode {
        PriceSourceMode::Inferred => Vec::new(),
        PriceSourceMode::Closed => closed::load_fresh(conn, now, StatsConfig::default().warm_min_trades).await?,
    };
    Ok(blend(inferred, closed, mode, guard_pct, now))
}
```

  and `StatsPriceSource::load` becomes:

```rust
    pub async fn load(conn: &DatabaseConnection, cache: &CacheState) -> Result<Self, Error> {
        let (mode, guard_pct) = source_settings();
        let rows = effective_stats(conn, mode, guard_pct, Utc::now()).await?;
        let tradable = cache.tradable_item();
        Ok(Self::from_effective(rows, |id| tradable.get_by(id).ok().map(|item| item.wfm_url)))
    }
```

  New imports: `chrono::{DateTime, Utc}`, `crate::collector::closed`, `crate::collector::stats::StatsConfig`, `crate::enums::PriceSourceMode`, `crate::trader::blend::{blend, Effective}`, `crate::utils::modules::states`. `load` keeps its signature, so its five callers (`trader/mod.rs`, `collector/runner.rs`, `commands/trader.rs`, `commands/analytics.rs`) are untouched.

  Run `cargo test -p qf_core --lib trader::price_source` — expected: PASS.

- [ ] **Step 6: The `FastDropGuard` tag.** In `trader/item.rs`, directly after each of the two `get_order_info(entry, OrderType::Buy, &ctx.orders, route);` / `get_order_info(entry, OrderType::Sell, &ctx.orders, route);` statements at the top of the buy function (near line 238) and the sell function (near line 385) — not the wish-list one near line 539 — add:

```rust
    if price.guarded {
        trade_operations.add("FastDropGuard"); // informational, spec §25 P5
    }
```

  Add this test beside `buying_posts_at_the_highest_buy_price_in_global_dry_run` in the same test module (it uses that module's `ctx_with`, `book`, `entry`, `item_info`, `price` and `route_for`):

```rust
    #[tokio::test]
    async fn a_guarded_price_tags_the_order_with_fast_drop_guard() {
        for guarded in [true, false] {
            let (_dir, ctx) = ctx_with(true, |_| {}).await;
            let live = book(&[20, 25], &[15, 17]);
            let mut e = entry("Buy", None, None);
            e.apply_market_info(&live);
            let info = ItemPriceInfo { guarded, ..price(30.0, true) };
            progress_buying(&ctx, &item_info(), &mut e, &info, &live, route_for(true, true)).await.unwrap();
            let log = ctx.orders.dry_log();
            assert_eq!(log.len(), 1);
            assert_eq!(log[0].reason.contains("FastDropGuard"), guarded);
        }
    }
```

  Run `cargo test -p qf_core --lib trader::item` — expected: PASS.

- [ ] **Step 6b: The trading-tax filter (spec §25 P6).** The tradable cache already carries each item's tax (`CacheTradableItem::trade_tax`). In `price_source.rs` add to `impl StatsPriceSource`:

```rust
    /// Fills `trading_tax` from the tradable cache; items the lookup does not know keep 0.
    pub fn with_trade_tax(mut self, tax_of: impl Fn(&str) -> Option<i64>) -> Self {
        for info in self.items.values_mut() {
            info.trading_tax = tax_of(&info.wfm_id).unwrap_or(0);
        }
        self
    }
```

  call it in `load` — `Ok(Self::from_effective(rows, |id| tradable.get_by(id).ok().map(|item| item.wfm_url)).with_trade_tax(|id| tradable.get_by(id).ok().map(|item| item.trade_tax)))` (cast with `as i64` if `trade_tax` is not already `i64`) — and add to `get_interesting_items`, after the shift filter:

```rust
        .filter(|i| is_disabled(wtb.trading_tax_cap) || i.trading_tax <= wtb.trading_tax_cap)
```

  Test, in the `price_source.rs` test module:

```rust
    #[test]
    fn the_trading_tax_cap_drops_expensive_to_trade_items() {
        let prices = source(vec![stats("cheap", "", 50.0, 50.0, 100.0), stats("dear", "", 50.0, 50.0, 100.0)]).with_trade_tax(|id| Some(if id == "dear" { 1_000_000 } else { 2_000 }));
        let mut settings = ItemSettings::default();
        assert_eq!(get_interesting_items(&settings, &prices).len(), 2, "the cap is disabled by default");
        settings.wtb.trading_tax_cap = 500_000;
        assert_eq!(get_interesting_items(&settings, &prices).into_iter().map(|i| i.wfm_id).collect::<Vec<_>>(), vec!["cheap"]);
    }
```

  Run `cargo test -p qf_core --lib trader::price_source` — expected: PASS.

- [ ] **Step 7: Market RPCs use the effective stats.** In `commands/market.rs` replace the `all_item_stats` import with `use crate::trader::price_source::{effective_stats, source_settings};` and both `let stats = all_item_stats(database()?).await?;` lines with:

```rust
    let (mode, guard_pct) = source_settings();
    let stats: Vec<_> = effective_stats(database()?, mode, guard_pct, Utc::now()).await?.into_iter().map(|e| e.stats).collect();
```

- [ ] **Step 8: Run the gate's Rust half.** `cargo test -p utils --lib && cargo test -p qf_core --lib && cargo test -p qf-server` — expected: PASS, no new warnings.

- [ ] **Step 9: Commit.**

```bash
git add crates/qf_core/src
git commit -m "feat(trader): blend closed-trade statistics into the price source behind a setting"
git push
```

---

### Task 4: The comparison RPC, the Price source tab, and the two settings in the UI

**Files:**
- Create: `crates/qf_core/src/trader/compare.rs`, `web/src/pages/market_data/Tabs/PriceSource/index.tsx`
- Modify: `crates/qf_core/src/trader/mod.rs` (add `pub mod compare;`), `crates/qf_core/src/collector/closed.rs` (`fetch_times`), `crates/qf_core/src/commands/market.rs`, `crates/qf_core/src/commands/rpc.rs`, `web/src/types/tauri.type.ts`, `web/src/api/market/index.ts`, `web/src/pages/market_data/Tabs/index.ts`, `web/src/pages/market_data/index.tsx`, `web/src/components/Forms/Settings/Tabs/LiveTrading/Tabs/General/index.tsx`, `web/public/lang/en.json`

**Interfaces:**
- Consumes (Tasks 1–3): `closed::{ClosedStats, RefreshStatus, load_fresh, refresh_status}`, `blend::{blend, Effective}`, `price_source::{all_item_stats, get_interesting_items, source_settings, StatsPriceSource}`, `PriceSourceMode`, `ItemSettings`.
- Produces:

```rust
// closed.rs
pub async fn fetch_times(conn: &DatabaseConnection) -> Result<HashMap<String, String>, Error>;   // item_id -> fetched_at, outcome 'ok' only

// trader/compare.rs
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PriceSourceRow { pub item_id: String, pub sub_type: String, pub name: String, pub wfm_url: String, pub inferred_volume: Option<f64>, pub inferred_moving_avg: Option<f64>, pub closed_volume: Option<f64>, pub closed_moving_avg: Option<f64>, pub closed_days: Option<usize>, pub week_price_shift: Option<f64>, pub profit: Option<f64>, pub warm_inferred: bool, pub warm_closed: bool, pub candidate_inferred: bool, pub candidate_closed: bool, pub guarded: bool, pub fetched_at: Option<String> }
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CandidateCounts { pub inferred: usize, pub closed: usize, pub both: usize }
pub struct ItemLookup { pub name: String, pub wfm_url: String, pub trade_tax: i64 }
pub fn compare(inferred: Vec<ItemStats>, closed: Vec<ClosedStats>, settings: &ItemSettings, guard_pct: i64, now: DateTime<Utc>, lookup: impl Fn(&str) -> Option<ItemLookup>, fetched_at: &HashMap<String, String>) -> (Vec<PriceSourceRow>, CandidateCounts);

// commands/market.rs
#[derive(Serialize)]
pub struct PriceSources { pub mode: PriceSourceMode, pub guard_pct: i64, pub refresh: RefreshStatus, pub candidates: CandidateCounts, pub rows: Vec<PriceSourceRow> }
pub async fn market_price_sources() -> Result<PriceSources, Error>;
```

- [ ] **Step 1: `compare` test first.** Create `trader/compare.rs`:

```rust
//! Both price bases side by side, with buy-candidate membership under each (spec §25 P8). Pure, no I/O.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::app::ItemSettings;
use crate::collector::closed::ClosedStats;
use crate::collector::stats::ItemStats;
use crate::enums::PriceSourceMode;
use crate::trader::blend::blend;
use crate::trader::price_source::{get_interesting_items, StatsPriceSource};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;

    fn inferred(id: &str, volume: f64, moving_avg: f64) -> ItemStats {
        ItemStats { item_id: id.into(), sub_type: String::new(), volume, avg_price: Some(moving_avg), moving_avg: Some(moving_avg), profit: Some(20.0), min_price: Some(1), max_price: Some(2), median: Some(moving_avg), history_days: 5, warm: false, updated_at: "2026-09-20T07:55:00Z".into() }
    }
    fn closed(id: &str, volume: f64, moving_avg: f64) -> ClosedStats {
        ClosedStats { item_id: id.into(), sub_type: String::new(), volume, moving_avg: Some(moving_avg), median: Some(moving_avg), avg_price: Some(moving_avg), min_price: Some(1), max_price: Some(2), week_price_shift: Some(1.0), days: 7, trades: (volume * 7.0) as i64, warm: true }
    }

    #[test]
    fn rows_carry_both_bases_and_candidate_membership_under_each_mode() {
        let mut settings = ItemSettings::default(); // volume_threshold 15, profit_threshold 10, avg_price_cap 600
        settings.wtb.volume_threshold = 15;
        let (rows, counts) = compare(
            vec![inferred("undercounted", 9.0, 70.0), inferred("busy", 40.0, 50.0), inferred("untradable", 99.0, 5.0)],
            vec![closed("undercounted", 30.0, 66.0), closed("busy", 60.0, 48.0), closed("closed_only", 25.0, 10.0)],
            &settings,
            10,
            parse_ts("2026-09-20T08:00:00Z").unwrap(),
            |id| (id != "untradable").then(|| ItemLookup { name: format!("Name {id}"), wfm_url: format!("{id}_slug"), trade_tax: 0 }),
            &HashMap::from([("busy".to_string(), "2026-09-20T01:00:00Z".to_string())]),
        );
        assert_eq!(rows.len(), 3, "untradable ids are dropped");
        let row = |id: &str| rows.iter().find(|r| r.item_id == id).unwrap();
        let u = row("undercounted");
        assert_eq!((u.inferred_volume, u.closed_volume, u.candidate_inferred, u.candidate_closed), (Some(9.0), Some(30.0), false, true));
        assert_eq!((u.warm_inferred, u.warm_closed, u.closed_days, u.name.as_str()), (false, true, Some(7), "Name undercounted"));
        assert_eq!((row("busy").candidate_inferred, row("busy").candidate_closed), (true, true));
        assert_eq!(row("busy").fetched_at.as_deref(), Some("2026-09-20T01:00:00Z"));
        let c = row("closed_only");
        assert_eq!((c.inferred_volume, c.profit, c.candidate_closed), (None, None, false), "no inferred profit, so the profit filter rejects it");
        assert_eq!(counts, CandidateCounts { inferred: 1, closed: 2, both: 1 });
    }
}
```

Add `pub mod compare;` to `trader/mod.rs`. Run `cargo test -p qf_core --lib trader::compare` — expected: FAIL to compile.

- [ ] **Step 2: Implement `compare`** above the tests:

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PriceSourceRow {
    pub item_id: String,
    pub sub_type: String,
    pub name: String,
    pub wfm_url: String,
    pub inferred_volume: Option<f64>,
    pub inferred_moving_avg: Option<f64>,
    pub closed_volume: Option<f64>,
    pub closed_moving_avg: Option<f64>,
    pub closed_days: Option<usize>,
    pub week_price_shift: Option<f64>,
    pub profit: Option<f64>,
    pub warm_inferred: bool,
    pub warm_closed: bool,
    pub candidate_inferred: bool,
    pub candidate_closed: bool,
    pub guarded: bool,
    pub fetched_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CandidateCounts {
    pub inferred: usize,
    pub closed: usize,
    pub both: usize,
}

pub struct ItemLookup {
    pub name: String,
    pub wfm_url: String,
    pub trade_tax: i64,
}

pub fn compare(
    inferred: Vec<ItemStats>,
    closed: Vec<ClosedStats>,
    settings: &ItemSettings,
    guard_pct: i64,
    now: DateTime<Utc>,
    lookup: impl Fn(&str) -> Option<ItemLookup>,
    fetched_at: &HashMap<String, String>,
) -> (Vec<PriceSourceRow>, CandidateCounts) {
    let candidates = |mode: PriceSourceMode| -> HashSet<String> {
        let source = StatsPriceSource::from_effective(blend(inferred.clone(), closed.clone(), mode, guard_pct, now), |id| lookup(id).map(|l| l.wfm_url))
            .with_trade_tax(|id| lookup(id).map(|l| l.trade_tax));
        get_interesting_items(settings, &source).into_iter().map(|i| i.uuid).collect()
    };
    let (as_inferred, as_closed) = (candidates(PriceSourceMode::Inferred), candidates(PriceSourceMode::Closed));
    let guarded: HashSet<(String, String)> = blend(inferred.clone(), closed.clone(), PriceSourceMode::Closed, guard_pct, now)
        .into_iter()
        .filter(|e| e.guarded)
        .map(|e| (e.stats.item_id, e.stats.sub_type))
        .collect();

    let mut keys: BTreeMap<(String, String), (Option<ItemStats>, Option<ClosedStats>)> = BTreeMap::new();
    for i in inferred {
        keys.entry((i.item_id.clone(), i.sub_type.clone())).or_default().0 = Some(i);
    }
    for c in closed {
        keys.entry((c.item_id.clone(), c.sub_type.clone())).or_default().1 = Some(c);
    }
    let rows: Vec<PriceSourceRow> = keys
        .into_iter()
        .filter_map(|((item_id, sub_type), (i, c))| {
            let item = lookup(&item_id)?;
            let uuid = format!("{item_id}:{sub_type}");
            Some(PriceSourceRow {
                name: item.name,
                wfm_url: item.wfm_url,
                inferred_volume: i.as_ref().map(|i| i.volume),
                inferred_moving_avg: i.as_ref().and_then(|i| i.moving_avg),
                closed_volume: c.as_ref().map(|c| c.volume),
                closed_moving_avg: c.as_ref().and_then(|c| c.moving_avg),
                closed_days: c.as_ref().map(|c| c.days),
                week_price_shift: c.as_ref().and_then(|c| c.week_price_shift),
                profit: i.as_ref().and_then(|i| i.profit),
                warm_inferred: i.as_ref().is_some_and(|i| i.warm),
                warm_closed: c.as_ref().is_some_and(|c| c.warm),
                candidate_inferred: as_inferred.contains(&uuid),
                candidate_closed: as_closed.contains(&uuid),
                guarded: guarded.contains(&(item_id.clone(), sub_type.clone())),
                fetched_at: fetched_at.get(&item_id).cloned(),
                item_id,
                sub_type,
            })
        })
        .collect();
    let counts = CandidateCounts {
        inferred: rows.iter().filter(|r| r.candidate_inferred).count(),
        closed: rows.iter().filter(|r| r.candidate_closed).count(),
        both: rows.iter().filter(|r| r.candidate_inferred && r.candidate_closed).count(),
    };
    (rows, counts)
}
```

`get_interesting_items` caps at 150, so each count is at most 150; that is the number the trader would actually work. Run `cargo test -p qf_core --lib trader::compare` — expected: PASS.

- [ ] **Step 3: `fetch_times` and the RPC.** In `closed.rs` add `use std::collections::HashMap;` (merge with the existing `HashSet` import) and:

```rust
pub async fn fetch_times(conn: &DatabaseConnection) -> Result<HashMap<String, String>, Error> {
    const C: &str = "ClosedStats:FetchTimes";
    conn.query_all(stmt("SELECT item_id, fetched_at FROM closed_fetch_state WHERE outcome = 'ok'", vec![]))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| Ok((r.try_get("", "item_id").map_err(|e| db_err(C, e))?, r.try_get("", "fetched_at").map_err(|e| db_err(C, e))?)))
        .collect()
}
```

Extend `refresh_once_writes_ok_missing_and_failed_states` with `assert_eq!(fetch_times(&conn).await.unwrap().keys().collect::<Vec<_>>(), vec!["item1"]);` placed right after the first `refresh_status` assertion. In `commands/market.rs` add:

```rust
use serde::Serialize;

use crate::collector::closed::{self, RefreshStatus};
use crate::collector::stats::StatsConfig;
use crate::enums::PriceSourceMode;
use crate::trader::compare::{self, CandidateCounts, ItemLookup, PriceSourceRow};
use crate::trader::price_source::all_item_stats;

#[derive(Serialize)]
pub struct PriceSources {
    pub mode: PriceSourceMode,
    pub guard_pct: i64,
    pub refresh: RefreshStatus,
    pub candidates: CandidateCounts,
    pub rows: Vec<PriceSourceRow>,
}

/// Both price bases side by side, whatever the current mode (spec §25 P8).
pub async fn market_price_sources() -> Result<PriceSources, Error> {
    let conn = database()?;
    let now = Utc::now();
    let (mode, guard_pct) = source_settings();
    let settings = states::app_state()?.settings.live_scraper.items.clone();
    let tradable = states::cache_client()?.tradable_item();
    let (rows, candidates) = compare::compare(
        all_item_stats(conn).await?,
        closed::load_fresh(conn, now, StatsConfig::default().warm_min_trades).await?,
        &settings,
        guard_pct,
        now,
        |id| tradable.get_by(id).ok().map(|item| ItemLookup { name: item.name, wfm_url: item.wfm_url, trade_tax: item.trade_tax }),
        &closed::fetch_times(conn).await?,
    );
    Ok(PriceSources { mode, guard_pct, refresh: closed::refresh_status(conn, now).await?, candidates, rows })
}
```

In `commands/rpc.rs` add the row `market_price_sources => market::market_price_sources {},` after `market_backfill_status`, and add `"market_price_sources"` to the name array in the test `market_commands_are_routable_and_validate_args` (`["market_overview", "market_movers", "market_warmup", "market_price_sources"]`). Run `cargo test -p qf_core --lib commands::rpc` — expected: PASS.

- [ ] **Step 4: Web types and API.** In `web/src/types/tauri.type.ts`, inside the general live-scraper settings interface (the one with `delete_conflicting_orders: boolean;`, near line 110) add:

```ts
    price_source: "inferred" | "closed";
    fast_drop_guard_pct: number;
```

and after `MarketBackfillStatus`:

```ts
  export interface MarketPriceSourceRow {
    item_id: string;
    sub_type: string;
    name: string;
    wfm_url: string;
    inferred_volume: number | null;
    inferred_moving_avg: number | null;
    closed_volume: number | null;
    closed_moving_avg: number | null;
    closed_days: number | null;
    week_price_shift: number | null;
    profit: number | null;
    warm_inferred: boolean;
    warm_closed: boolean;
    candidate_inferred: boolean;
    candidate_closed: boolean;
    guarded: boolean;
    fetched_at: string | null;
  }
  export interface MarketPriceSources {
    mode: "inferred" | "closed";
    guard_pct: number;
    refresh: { active: number; ok: number; missing: number; failed: number; stale: number; oldest_fetched_at: string | null };
    candidates: { inferred: number; closed: number; both: number };
    rows: MarketPriceSourceRow[];
  }
```

In `web/src/api/market/index.ts` add:

```ts
  priceSources() {
    return this.client.sendInvoke<TauriTypes.MarketPriceSources>("market_price_sources");
  }
```

- [ ] **Step 5: The tab.** Create `web/src/pages/market_data/Tabs/PriceSource/index.tsx`:

```tsx
import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Badge, Group, Loader, Paper, SimpleGrid, Switch, Text, TextInput } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { num, sortRows } from "@utils/sortRows";
import { DataTable, DataTableSortStatus } from "mantine-datatable";
import { useMemo, useState } from "react";
import { TauriTypes } from "$types";

const PAGE = 50;
type Row = TauriTypes.MarketPriceSourceRow;

/** Candidate membership differs, or the two moving averages are more than 10 % apart (spec §25 P8). */
function differs(r: Row) {
  if (r.candidate_inferred !== r.candidate_closed) return true;
  if (r.inferred_moving_avg == null || r.closed_moving_avg == null || r.closed_moving_avg === 0) return false;
  return Math.abs(r.inferred_moving_avg - r.closed_moving_avg) / r.closed_moving_avg > 0.1;
}

export function PriceSourcePanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`market_data.tabs.price_source.${key}`, context);
  const [search, setSearch] = useState("");
  const [onlyDiffs, setOnlyDiffs] = useState(true);
  const [page, setPage] = useState(1);
  const [sort, setSort] = useState<DataTableSortStatus<Row>>({ columnAccessor: "closed_volume", direction: "desc" });
  const { data, isPending } = useQuery({ queryKey: ["market_price_sources"], queryFn: () => api.market.priceSources(), enabled: !!isActive });
  const rows = useMemo(() => {
    const filtered = (data?.rows ?? []).filter((r) => (!onlyDiffs || differs(r)) && r.name.toLowerCase().includes(search.toLowerCase()));
    return sortRows(filtered, sort);
  }, [data, onlyDiffs, search, sort]);
  if (isPending) return <Loader size="sm" mt="md" />;
  if (!data) return null;
  const yesNo = (value: boolean) => (value ? <Badge color="green">{t("yes")}</Badge> : <Badge color="gray">{t("no")}</Badge>);

  return (
    <>
      <Text size="sm" mt="md">
        {t("mode", { mode: t(`modes.${data.mode}`), guard: data.guard_pct })}
      </Text>
      <Text size="sm" c="dimmed">
        {t("refresh", data.refresh)}
      </Text>
      <SimpleGrid cols={{ base: 1, md: 3 }} mt="sm">
        {(["inferred", "closed", "both"] as const).map((key) => (
          <Paper withBorder p="sm" key={key}>
            <Text size="xs" c="dimmed">
              {t(`candidates.${key}`)}
            </Text>
            <Text fw={700} size="lg">
              {data.candidates[key]}
            </Text>
          </Paper>
        ))}
      </SimpleGrid>
      <Group mt="md" align="end">
        <TextInput
          label={t("search")}
          value={search}
          onChange={(e) => {
            setSearch(e.currentTarget.value);
            setPage(1);
          }}
        />
        <Switch
          label={t("only_diffs")}
          checked={onlyDiffs}
          onChange={(e) => {
            setOnlyDiffs(e.currentTarget.checked);
            setPage(1);
          }}
        />
      </Group>
      <DataTable
        mt="sm"
        withTableBorder
        striped
        records={rows.slice((page - 1) * PAGE, page * PAGE)}
        idAccessor={(r) => `${r.item_id}:${r.sub_type}`}
        totalRecords={rows.length}
        recordsPerPage={PAGE}
        page={page}
        onPageChange={setPage}
        sortStatus={sort}
        onSortStatusChange={setSort}
        columns={[
          { accessor: "name", title: t("columns.name"), sortable: true, render: (r) => (r.sub_type ? `${r.name} (${r.sub_type})` : r.name) },
          { accessor: "inferred_volume", title: t("columns.inferred_volume"), sortable: true, render: (r) => num(r.inferred_volume, 1) },
          { accessor: "closed_volume", title: t("columns.closed_volume"), sortable: true, render: (r) => num(r.closed_volume, 1) },
          { accessor: "inferred_moving_avg", title: t("columns.inferred_moving_avg"), sortable: true, render: (r) => num(r.inferred_moving_avg, 1) },
          { accessor: "closed_moving_avg", title: t("columns.closed_moving_avg"), sortable: true, render: (r) => num(r.closed_moving_avg, 1) },
          { accessor: "week_price_shift", title: t("columns.week_price_shift"), sortable: true, render: (r) => num(r.week_price_shift, 1) },
          { accessor: "profit", title: t("columns.profit"), sortable: true, render: (r) => num(r.profit, 0) },
          { accessor: "closed_days", title: t("columns.closed_days"), sortable: true },
          { accessor: "candidate_inferred", title: t("columns.candidate_inferred"), sortable: true, render: (r) => yesNo(r.candidate_inferred) },
          { accessor: "candidate_closed", title: t("columns.candidate_closed"), sortable: true, render: (r) => yesNo(r.candidate_closed) },
          { accessor: "guarded", title: t("columns.guarded"), sortable: true, render: (r) => (r.guarded ? <Badge color="orange">{t("yes")}</Badge> : null) },
        ]}
      />
    </>
  );
}
```

`num(value, digits)` is the phase 6b helper in `web/src/utils/sortRows.ts` (`null` renders as `—`). Add `export * from "./PriceSource";` to `web/src/pages/market_data/Tabs/index.ts`. In `web/src/pages/market_data/index.tsx` add `PriceSourcePanel` to the import and this entry after the `warmup` entry:

```tsx
      { id: "price_source", label: useTranslateTabs("price_source.title"), component: (isActive: boolean) => <PriceSourcePanel isActive={isActive} /> },
```

- [ ] **Step 6: The two settings in the General tab.** In `web/src/components/Forms/Settings/Tabs/LiveTrading/Tabs/General/index.tsx` add `NumberInput` to the `@mantine/core` import (`Select` is already imported) and, directly after the closing `</Group>` of the checkbox group that holds `delete_conflicting_orders`, add:

```tsx
          <Group gap={"md"} mt={25} align="end">
            <Tooltip label={useTranslateFormFields("price_source.tooltip")}>
              <Select
                label={useTranslateFormFields("price_source.label")}
                allowDeselect={false}
                data={[
                  { value: "inferred", label: useTranslateFormFields("price_source.options.inferred") },
                  { value: "closed", label: useTranslateFormFields("price_source.options.closed") },
                ]}
                value={form.values.live_scraper.general.price_source}
                onChange={(value) => form.setFieldValue(getFieldPath("general.price_source"), value ?? "inferred")}
              />
            </Tooltip>
            <Tooltip label={useTranslateFormFields("fast_drop_guard_pct.tooltip")}>
              <NumberInput
                label={useTranslateFormFields("fast_drop_guard_pct.label")}
                min={-1}
                max={90}
                value={form.values.live_scraper.general.fast_drop_guard_pct}
                onChange={(value) => form.setFieldValue(getFieldPath("general.fast_drop_guard_pct"), Number(value))}
              />
            </Tooltip>
          </Group>
```

- [ ] **Step 7: `en.json`, two targeted insertions** (a Python script with exact anchor strings, as in earlier phases; never rewrite the file). Block A goes directly before the line `                "delete_conflicting_orders": {` (16 spaces of indent, inside `components.forms.settings.tabs.live_scraper.general.fields`):

```json
                "price_source": {
                  "label": "Price source",
                  "tooltip": "Inferred: the collector's own statistics. Closed: warframe.market's closed-trade statistics blended with the collector's live data. Change it only while in dry-run.",
                  "options": {
                    "inferred": "Inferred (collector)",
                    "closed": "Closed trades (warframe.market)"
                  }
                },
                "fast_drop_guard_pct": {
                  "label": "Fast-drop guard (%)",
                  "tooltip": "Closed mode only. When the collector's 48-hour average is this far below the closed 7-day average, the lower figure is used. -1 turns it off."
                },
```

Block B goes directly after the `warmup` block's closing brace inside `pages.market_data.tabs` (turn that `}` into `},` and insert after it, 8 spaces of indent):

```json
        "price_source": {
          "title": "Price source",
          "mode": "Trader is using: {{mode}} · fast-drop guard {{guard}} %",
          "modes": { "inferred": "inferred statistics", "closed": "closed-trade statistics" },
          "refresh": "Closed statistics: {{ok}} of {{active}} items fetched, {{missing}} missing, {{failed}} failed, {{stale}} waiting for today's refresh",
          "candidates": { "inferred": "Buy candidates (inferred)", "closed": "Buy candidates (closed)", "both": "In both" },
          "search": "Search",
          "only_diffs": "Differences only",
          "yes": "Yes",
          "no": "No",
          "columns": {
            "name": "Item",
            "inferred_volume": "Volume/day (inferred)",
            "closed_volume": "Volume/day (closed)",
            "inferred_moving_avg": "Moving avg (inferred)",
            "closed_moving_avg": "Moving avg (closed)",
            "week_price_shift": "Week shift",
            "profit": "Profit",
            "closed_days": "Closed days",
            "candidate_inferred": "Candidate (inferred)",
            "candidate_closed": "Candidate (closed)",
            "guarded": "Guard"
          }
        }
```

Verify: `python3 -c "import json;json.load(open('web/public/lang/en.json'))"` and `git diff --stat web/public/lang/en.json` shows only insertions plus the one `}` → `},` line.

- [ ] **Step 8: Full gate.** `cargo test -p utils --lib && cargo test -p qf_core --lib && cargo test -p qf-server && python3 scripts/check-rpc-commands.py && (cd web && pnpm build)` — expected: all green, no new warnings.

- [ ] **Step 9: Commit.**

```bash
git add crates/qf_core/src web/src web/public/lang/en.json
git commit -m "feat(web): compare the inferred and closed price bases and expose the price source setting"
git push
```

---

### Task 5: Deploy, accept, and update the runbook

**Files:**
- Modify: `docs/GO-LIVE-RUNBOOK.md`
- Create: `docs/PHASE-6D-ACCEPTANCE.md`

This task runs only after the controller's final whole-branch review and fix wave. The deploy needs the user's explicit go-ahead and is run by the user with `!` commands (agent rsync to ockohome is denied in auto mode); the agent reads the results over ssh (`docker compose ps`, `docker compose logs quantframe-server`, `curl /healthz`) and the user checks the browser.

- [ ] **Step 1: Runbook.** In `docs/GO-LIVE-RUNBOOK.md` §1 Pre-flight add, after the Auto Delete item:

```markdown
- [ ] **Settings → Live Scraper → General → Price source** is the one you have reviewed. `Inferred` needs nothing more. `Closed trades` needs at least **48 h of dry-run in that mode** with the Dry-run log Summary reviewed, and **Market Data → Price source** showing the closed statistics fetched for nearly all items with `failed` near 0. Change the source only while in dry-run.
```

and in §4 Rollback add as a new first step: `0. If the problem appeared after switching **Price source** to Closed trades, set it back to **Inferred**; it takes effect on the next trader cycle, with no restart.` Commit: `docs: add the price source pre-flight and rollback to the go-live runbook`.

- [ ] **Step 2: Hand the user the deploy commands** (the established pattern; run from the worktree root). First the deletion preview, which must list nothing beyond files this phase removed (it removes none):

```bash
! cd ~/Projects/Personal/quantframe-server-phase-6d && rsync -a --delete --dry-run --itemize-changes --exclude .git --exclude target --exclude web/node_modules --exclude web/dist --exclude secrets --exclude .env --exclude .superpowers --exclude backups --exclude 'crates/*/logs' ./ christopher@ockohome:~/stacks/quantframe-server/ | grep deleting
```

Then the real copy and the rebuild:

```bash
! cd ~/Projects/Personal/quantframe-server-phase-6d && rsync -a --delete --exclude .git --exclude target --exclude web/node_modules --exclude web/dist --exclude secrets --exclude .env --exclude .superpowers --exclude backups --exclude 'crates/*/logs' ./ christopher@ockohome:~/stacks/quantframe-server/
! ssh -o ClearAllForwardings=yes christopher@ockohome 'cd ~/stacks/quantframe-server && docker compose config -q && docker compose up -d --build && sleep 25 && docker compose ps && docker compose logs --tail 40 quantframe-server'
```

Never add `--delete-excluded`: it would remove `secrets/`, `backups/` and `.env` on the server. The build takes several minutes, so the second command may outlive the 120 s foreground limit and finish in the background; read its saved output. Expected: the container is `healthy`, the log shows the new migration applied and `Collector` `Started`, and there is no panic. Successful closed-statistics fetches are silent; only failures log a `ClosedStats` warning, so progress is read from the Price source tab.

- [ ] **Step 3: Acceptance checks (spec §25 P11), recorded as a table in `docs/PHASE-6D-ACCEPTANCE.md`:**
  1. With `price_source = inferred` (the default after deploy) the Dry-run log keeps its cadence and item count from before the deploy; the Price source tab's status line shows `ok` climbing by about 6 a minute.
  2. The user presses **Import 90-day history** on the Collector tab; about 70 minutes later it reads Finished and the Price source tab shows `ok` near 3 840, `failed` 0 or near it.
  3. Ash Prime Set on the tab: closed volume and moving average agree with its warframe.market statistics page for the last 7 days.
  4. **Calibration:** five items the user trades, closed `volume` and `moving_avg` against the desktop Quantframe app's figures for the same items. Record both numbers per item. A systematic factor on volume is a spec error: stop, report it, do not switch.
  5. The candidate counts are plausible: `closed` is larger than `inferred`, `both` is most of `inferred`.
  6. In dry-run, the user sets Price source to Closed trades and saves; the next trader cycle's `Progress: n/N` total follows the `closed` candidate count plus stock and wish-list items; `FastDropGuard` appears in few or no Dry-run log reasons; the Warm-up tab's warm count jumps from 0 to the closed-warm count.
  7. Setting it back to Inferred restores the previous cycle size on the next cycle.
  8. Next day after 00:30 UTC: the log shows `[ClosedStats] Pass complete` within about 11 h and hot-set items show a `fetched_at` from today within the first hour.

- [ ] **Step 4: Record and hand back.** Write `docs/PHASE-6D-ACCEPTANCE.md` in the form of `docs/PHASE-6C-ACCEPTANCE.md` (deployed commit, checks table with Pass/Fail and evidence, follow-ups including every deferred minor from the task reviews, and the still-open list carried forward by number). Commit `docs: record phase 6d closed price source acceptance`, push, and report to the controller. The controller merges to `main` only after this record is committed and the gate is green on the branch tip.

---

## Self-Review (done while writing)

- **Spec coverage:** P1 → Task 1 Steps 1, 5; P2 → Task 2 (loop, lane, stale order, failed retry, pass log, retention, import button); P3 → Task 1 `aggregate`/`load_fresh`; P4 → Task 3 `blend`; P5 → Task 3 guard + Step 6 tag; P6 → Task 3 Steps 4–6b; P7 → Task 3 Steps 1, 5, 7 and Task 4 Step 6; P8 → Task 4; P9 → nothing built, by design; P10 → tests in Tasks 1–4; P11 → Task 5.
- **Identity in the default mode:** `blend(.., Inferred, ..)` returns the inferred rows untouched (`inferred_mode_is_the_identity`), `from_stats` delegates to `from_effective` with no shift and no guard, the shift filter passes items without a shift, and the tax cap is disabled by default — so with default settings every trader input equals `main`'s.
- **Type consistency:** `ClosedDay` (8 fields) is the only type shared between `backfill.rs` and `closed.rs`; `ClosedStats` feeds `blend` and `compare`; `Effective` feeds `from_effective`; `fetch_with_retries(source, limiter, lane, slug)` has one signature used by `run` (`Lane::Hot`) and `refresh_once` (`Lane::Cold`); `RefreshStatus`, `CandidateCounts` and `PriceSourceRow` serialize with the exact field names the TypeScript interfaces declare.
