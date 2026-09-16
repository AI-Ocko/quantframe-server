# Phase 6c: Market History Backfill — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task with Opus implementers and a reviewer between tasks. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A button on the Collector tab that imports warframe.market's 90-day closed-trade daily statistics for every tradable item into `item_stats_daily`, filling only days the collector has not produced, so Movers and the Price History daily chart have history from day one.

**Architecture:** A new `collector::backfill` module parses the v1 statistics body into daily rows keyed by the collector's own sub-type key, inserts them with `INSERT OR IGNORE`, and runs the whole item list through the limiter's Hot lane with the collector's retry rule, keeping a process-wide status. Two RPCs start the job and read its status; the Collector tab shows a button and a status line and polls only while running.

**Tech Stack:** Rust (tokio, reqwest, serde_json, sea-orm raw SQL over SQLite), React 19 + Mantine 9.4.1 + TanStack Query 5, pnpm 11.3.0.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md` — read **§24 (K1–K7)** first; §15 B2/B10 for the collector's HTTP and retry rules. §24 takes precedence.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-phase-6c`, branch `phase-6c-market-backfill` (from `main` at `dc9f434`). Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target`.
- **Tests:** `cargo test -p utils --lib`, `cargo test -p qf_core --lib`, `cargo test -p qf-server`; never bare `--workspace`. Web: `python3 scripts/check-rpc-commands.py` and `(cd web && pnpm build)`. Output pristine.
- **Exact values (K1–K4):** endpoint `GET https://api.warframe.market/v1/items/{slug}/statistics`, headers `Platform: pc`, `Language: en`; series `payload.statistics_closed["90days"]`; `sub_type = sub_type_key(mod_rank, None, subtype, None, None)`; `day = datetime[..10]`; `min_price`/`max_price` rounded to `i64`; `INSERT OR IGNORE INTO item_stats_daily`; limiter lane `Hot`; two retries with `fetch.rs`'s jitter; 404 = missing, exhausted retries = failed; progress log every 500 items; RPCs `market_backfill_start {}` and `market_backfill_status {}`; status shape `{ state, started_at, finished_at, items_total, items_done, days_inserted, items_missing, items_failed, last_error }` with `state` one of `idle | running | done | failed`; web polls every 5 s only while `running`; strings under `pages.market_data.tabs.collector.backfill.*`.
- **Never** write to any table other than `item_stats_daily`; never `INSERT OR REPLACE`.
- **`en.json`** by targeted text insertion only; confirm it parses and the diff is one block.
- **Commits:** conventional commits ending with exactly `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`; `git push` after each task. Never touch `main`.
- Tasks 1–3 do not touch ockohome; Task 4 is deployed by the user with `!` commands and checked in the browser.

## File Structure (end of phase 6c)

```
crates/qf_core/src/collector/backfill.rs              NEW  ClosedDay, parse_statistics, insert_missing, StatisticsSource, HttpStatisticsSource, BackfillStatus, run, tests
crates/qf_core/src/collector/mod.rs                    MOD  pub mod backfill;
crates/qf_core/tests/fixtures/statistics_small.json    NEW  fixture body
crates/qf_core/src/commands/market.rs                  MOD  market_backfill_start, market_backfill_status
crates/qf_core/src/commands/rpc.rs                     MOD  two rows + test
web/src/api/market/index.ts                            MOD  backfillStart, backfillStatus
web/src/types/tauri.type.ts                            MOD  MarketBackfillStatus
web/src/pages/market_data/Tabs/Collector/index.tsx     MOD  Historical data block
web/public/lang/en.json                                MOD  collector.backfill.*
docs/PHASE-6C-ACCEPTANCE.md                            NEW  (Task 4)
```

---

### Task 1: `collector::backfill` — parse and insert

**Files:**
- Create: `crates/qf_core/src/collector/backfill.rs`, `crates/qf_core/tests/fixtures/statistics_small.json`
- Modify: `crates/qf_core/src/collector/mod.rs` (add `pub mod backfill;`)

**Interfaces:**
- Consumes: `super::orders::sub_type_key`, `super::store::{exec, tests::setup}`, `super::{db_err, stmt}`.
- Produces:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ClosedDay { pub sub_type: String, pub day: String, pub volume: i64, pub median: Option<f64>, pub min_price: Option<i64>, pub max_price: Option<i64> }
pub fn parse_statistics(body: &str) -> Result<Vec<ClosedDay>, Error>;
pub async fn insert_missing(conn: &DatabaseConnection, item_id: &str, days: &[ClosedDay]) -> Result<u64, Error>;  // rows actually inserted
```

- [ ] **Step 1: Fixture** `crates/qf_core/tests/fixtures/statistics_small.json` (trimmed to what the parser reads; extra fields are allowed and ignored):

```json
{
  "payload": {
    "statistics_closed": {
      "48hours": [
        {"datetime": "2026-09-16T16:00:00.000+00:00", "volume": 3, "min_price": 69.0, "max_price": 69.0, "median": 69.0, "mod_rank": 0}
      ],
      "90days": [
        {"datetime": "2026-09-14T00:00:00.000+00:00", "volume": 39, "min_price": 47.0, "max_price": 50.4, "median": 50.0, "mod_rank": 10},
        {"datetime": "2026-09-15T00:00:00.000+00:00", "volume": 12, "min_price": 20.0, "max_price": 25.0, "median": 22.0, "mod_rank": 0},
        {"datetime": "2026-09-15T00:00:00.000+00:00", "volume": 15, "min_price": 10.0, "max_price": 12.0, "median": 11.0, "subtype": "intact"},
        {"datetime": "2026-09-15T00:00:00.000+00:00", "volume": 43, "min_price": 66.0, "max_price": 70.0, "median": 69.0}
      ]
    },
    "statistics_live": {"48hours": [], "90days": []}
  }
}
```

- [ ] **Step 2: Write the failing tests** in `backfill.rs`:

```rust
//! One-off import of warframe.market's 90-day closed-trade statistics (spec §24).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::store::{exec, tests::setup};

    const SMALL: &str = include_str!("../../tests/fixtures/statistics_small.json");

    #[test]
    fn parses_the_90_day_series_with_the_collector_sub_type_keys() {
        let days = parse_statistics(SMALL).unwrap();
        assert_eq!(
            days,
            vec![
                ClosedDay { sub_type: "rank=10".into(), day: "2026-09-14".into(), volume: 39, median: Some(50.0), min_price: Some(47), max_price: Some(50) },
                ClosedDay { sub_type: "rank=0".into(), day: "2026-09-15".into(), volume: 12, median: Some(22.0), min_price: Some(20), max_price: Some(25) },
                ClosedDay { sub_type: "subtype=intact".into(), day: "2026-09-15".into(), volume: 15, median: Some(11.0), min_price: Some(10), max_price: Some(12) },
                ClosedDay { sub_type: String::new(), day: "2026-09-15".into(), volume: 43, median: Some(69.0), min_price: Some(66), max_price: Some(70) },
            ]
        );
    }

    #[test]
    fn rejects_bodies_without_the_series() {
        assert!(parse_statistics("{}").is_err());
        assert!(parse_statistics(r#"{"payload":{"statistics_closed":{"90days":"nope"}}}"#).is_err());
        assert!(parse_statistics("not json").is_err());
        assert!(parse_statistics(r#"{"payload":{"statistics_closed":{"90days":[]}}}"#).unwrap().is_empty());
    }

    #[tokio::test]
    async fn insert_missing_adds_new_days_and_never_overwrites_collector_days() {
        let (_dir, conn) = setup().await;
        exec(&conn, "Test", "INSERT INTO item_stats_daily (item_id, sub_type, day, volume, median, min_price, max_price) VALUES ('item1', 'rank=0', '2026-09-15', 7, 99.0, 90, 100)", vec![]).await.unwrap();
        let days = parse_statistics(SMALL).unwrap();
        let inserted = insert_missing(&conn, "item1", &days).await.unwrap();
        assert_eq!(inserted, 3, "the rank=0 2026-09-15 row already existed");
        let kept = conn
            .query_one(stmt("SELECT volume, median FROM item_stats_daily WHERE item_id = 'item1' AND sub_type = 'rank=0' AND day = '2026-09-15'", vec![]))
            .await.unwrap().unwrap();
        assert_eq!(kept.try_get::<i64>("", "volume").unwrap(), 7);
        assert_eq!(kept.try_get::<f64>("", "median").unwrap(), 99.0);
        assert_eq!(insert_missing(&conn, "item1", &days).await.unwrap(), 0, "idempotent");
    }
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib collector::backfill`
Expected: compile errors (`ClosedDay`, `parse_statistics`, `insert_missing` missing). Add `pub mod backfill;` to `collector/mod.rs` first.

- [ ] **Step 4: Implement** above the tests:

```rust
use serde::Deserialize;
use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::{get_location, Error};

use super::orders::sub_type_key;
use super::{db_err, stmt};

#[derive(Debug, Clone, PartialEq)]
pub struct ClosedDay {
    pub sub_type: String,
    pub day: String,
    pub volume: i64,
    pub median: Option<f64>,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
}

#[derive(Deserialize)]
struct Body {
    payload: Payload,
}
#[derive(Deserialize)]
struct Payload {
    statistics_closed: Closed,
}
#[derive(Deserialize)]
struct Closed {
    #[serde(rename = "90days")]
    days90: Vec<Row>,
}
#[derive(Deserialize)]
struct Row {
    datetime: String,
    volume: i64,
    min_price: Option<f64>,
    max_price: Option<f64>,
    median: Option<f64>,
    mod_rank: Option<i64>,
    subtype: Option<String>,
}

/// The `90days` closed-trade series as `item_stats_daily` rows keyed by the collector's sub-type key (spec §24 K1).
pub fn parse_statistics(body: &str) -> Result<Vec<ClosedDay>, Error> {
    const C: &str = "Backfill:Parse";
    let body: Body = serde_json::from_str(body).map_err(|e| Error::new(C, e.to_string(), get_location!()))?;
    Ok(body
        .payload
        .statistics_closed
        .days90
        .into_iter()
        .map(|r| ClosedDay {
            sub_type: sub_type_key(r.mod_rank, None, r.subtype.as_deref(), None, None),
            day: r.datetime.chars().take(10).collect(),
            volume: r.volume,
            median: r.median,
            min_price: r.min_price.map(|p| p.round() as i64),
            max_price: r.max_price.map(|p| p.round() as i64),
        })
        .collect())
}

/// Inserts the days the collector has not produced; existing `(item_id, sub_type, day)` rows are left alone (spec §24 K2).
pub async fn insert_missing(conn: &DatabaseConnection, item_id: &str, days: &[ClosedDay]) -> Result<u64, Error> {
    const C: &str = "Backfill:Insert";
    let mut inserted = 0;
    for d in days {
        let result = conn
            .execute(stmt(
                "INSERT OR IGNORE INTO item_stats_daily (item_id, sub_type, day, volume, median, min_price, max_price) VALUES (?, ?, ?, ?, ?, ?, ?)",
                vec![item_id.into(), d.sub_type.clone().into(), d.day.clone().into(), d.volume.into(), d.median.into(), d.min_price.into(), d.max_price.into()],
            ))
            .await
            .map_err(|e| db_err(C, e))?;
        inserted += result.rows_affected();
    }
    Ok(inserted)
}
```

If `serde_json::from_str` accepts `"90days": "nope"` (it will not, since `days90` is a `Vec`), the second rejection assertion passes for free; keep it.

- [ ] **Step 5: Run the tests**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib collector::backfill`
Expected: 3 passed.

- [ ] **Step 6: Commit and push**

```bash
git add crates/qf_core/src/collector/backfill.rs crates/qf_core/src/collector/mod.rs crates/qf_core/tests/fixtures/statistics_small.json
git commit -m "feat(collector): parse warframe.market 90-day statistics and insert missing daily rows

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push -u origin phase-6c-market-backfill
```

---

### Task 2: The job, its status, the HTTP source and the two RPCs

**Files:**
- Modify: `crates/qf_core/src/collector/backfill.rs`, `crates/qf_core/src/commands/market.rs`, `crates/qf_core/src/commands/rpc.rs`

**Interfaces:**
- Consumes: Task 1; `crate::market::limiter::{global, Lane, Limiter}` (`acquire(lane).await`, `report_429()`); `super::fetch::FetchError` (`NotFound | RateLimited | Transient(String)`) and its `MAX_RETRIES` + jitter rule (copy the 500–1500 ms jitter helper; do not change `fetch.rs`); `states::cache_client()?.tradable_item().get_items()` → `Vec<CacheTradableItem>` with `wfm_id`, `wfm_url`; `utils::info`/`warning` logging with component `"Backfill"`.
- Produces:

```rust
pub trait StatisticsSource: Send + Sync { fn fetch<'a>(&'a self, slug: &'a str) -> Pin<Box<dyn Future<Output = Result<String, FetchError>> + Send + 'a>>; }
pub struct HttpStatisticsSource { http: reqwest::Client, base_url: String }   // new(http, base_url); default base "https://api.warframe.market/v1"
#[derive(Debug, Clone, Serialize, PartialEq)] #[serde(rename_all = "lowercase")] pub enum BackfillState { Idle, Running, Done, Failed }
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct BackfillStatus { pub state: BackfillState, pub started_at: Option<String>, pub finished_at: Option<String>, pub items_total: i64, pub items_done: i64, pub days_inserted: i64, pub items_missing: i64, pub items_failed: i64, pub last_error: Option<String> }
pub fn status() -> BackfillStatus;                                    // snapshot of the process-wide status
pub async fn run(conn: &DatabaseConnection, source: &dyn StatisticsSource, limiter: &Limiter, items: Vec<(String, String)>) -> BackfillStatus;  // (item_id, slug); drives the process-wide status and returns the final one
pub fn start(conn: DatabaseConnection, items: Vec<(String, String)>) -> BackfillStatus;  // spawns run() with the HTTP source and the global limiter unless already running
// commands/market.rs
pub async fn market_backfill_start() -> Result<BackfillStatus, Error>;
pub async fn market_backfill_status() -> Result<BackfillStatus, Error>;
```

`BackfillState` must implement `Default` as `Idle`.

- [ ] **Step 1: Write the failing job test** in `backfill.rs` `tests` (add the imports the test needs at the top of the module):

```rust
    struct Scripted(std::sync::Mutex<std::collections::HashMap<String, std::collections::VecDeque<Result<String, FetchError>>>>);
    impl StatisticsSource for Scripted {
        fn fetch<'a>(&'a self, slug: &'a str) -> Pin<Box<dyn Future<Output = Result<String, FetchError>> + Send + 'a>> {
            let next = self.0.lock().unwrap().get_mut(slug).and_then(|q| q.pop_front()).unwrap_or(Err(FetchError::Transient("unscripted".into())));
            Box::pin(async move { next })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn run_counts_inserted_missing_and_failed_items_and_reports_progress() {
        let (_dir, conn) = setup().await;
        let scripted = Scripted(std::sync::Mutex::new(std::collections::HashMap::from([
            ("ok".to_string(), std::collections::VecDeque::from([Ok(SMALL.to_string())])),
            ("gone".to_string(), std::collections::VecDeque::from([Err(FetchError::NotFound)])),
            ("flaky".to_string(), std::collections::VecDeque::from([Err(FetchError::Transient("boom".into())), Ok(SMALL.to_string())])),
            ("dead".to_string(), std::collections::VecDeque::from([Err(FetchError::Transient("1".into())), Err(FetchError::Transient("2".into())), Err(FetchError::Transient("3".into()))])),
        ])));
        let limiter = Limiter::new_for_tests();
        let items = vec![("id-ok".to_string(), "ok".to_string()), ("id-gone".to_string(), "gone".to_string()), ("id-flaky".to_string(), "flaky".to_string()), ("id-dead".to_string(), "dead".to_string())];
        let final_status = run(&conn, &scripted, &limiter, items).await;
        assert_eq!(final_status.state, BackfillState::Done);
        assert_eq!((final_status.items_total, final_status.items_done), (4, 4));
        assert_eq!(final_status.days_inserted, 8, "4 rows for ok + 4 for flaky");
        assert_eq!((final_status.items_missing, final_status.items_failed), (1, 1));
        assert!(final_status.started_at.is_some() && final_status.finished_at.is_some());
        assert_eq!(status(), final_status, "the process-wide status holds the final snapshot");
    }
```

Check how the limiter is constructed in `crates/qf_core/src/market/limiter.rs` tests (there may be a `Limiter::new(...)` or a test constructor; use what exists and name it correctly instead of `new_for_tests`).

- [ ] **Step 2: Run it to verify it fails**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib collector::backfill::tests::run_counts`
Expected: compile errors (`StatisticsSource`, `run`, `status`, `BackfillState` missing).

- [ ] **Step 3: Implement** in `backfill.rs`:

```rust
use std::future::Future;
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use serde::Serialize;
use utils::{info, warning, LoggerOptions};

use super::fetch::FetchError;
use super::ts;
use crate::market::limiter::{Lane, Limiter};

pub type StatsFuture<'a> = Pin<Box<dyn Future<Output = Result<String, FetchError>> + Send + 'a>>;

pub trait StatisticsSource: Send + Sync {
    fn fetch<'a>(&'a self, slug: &'a str) -> StatsFuture<'a>;
}

/// Unauthenticated client for the public v1 statistics endpoint (spec §24, the second permitted v1 call).
pub struct HttpStatisticsSource {
    http: reqwest::Client,
    base_url: String,
}

impl HttpStatisticsSource {
    pub fn new(http: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self { http, base_url: base_url.into() }
    }
}

impl StatisticsSource for HttpStatisticsSource {
    fn fetch<'a>(&'a self, slug: &'a str) -> StatsFuture<'a> {
        Box::pin(async move {
            let url = format!("{}/items/{}/statistics", self.base_url, slug);
            let response = self.http.get(&url).header("Platform", "pc").header("Language", "en").send().await.map_err(|e| FetchError::Transient(e.to_string()))?;
            match response.status().as_u16() {
                200 => {}
                404 => return Err(FetchError::NotFound),
                429 => return Err(FetchError::RateLimited),
                code => return Err(FetchError::Transient(format!("HTTP {code} for {url}"))),
            }
            response.text().await.map_err(|e| FetchError::Transient(e.to_string()))
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum BackfillState {
    #[default]
    Idle,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct BackfillStatus {
    pub state: BackfillState,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub items_total: i64,
    pub items_done: i64,
    pub days_inserted: i64,
    pub items_missing: i64,
    pub items_failed: i64,
    pub last_error: Option<String>,
}

static STATUS: OnceLock<Mutex<BackfillStatus>> = OnceLock::new();

fn status_cell() -> &'static Mutex<BackfillStatus> {
    STATUS.get_or_init(|| Mutex::new(BackfillStatus::default()))
}

pub fn status() -> BackfillStatus {
    status_cell().lock().unwrap().clone()
}

fn update(f: impl FnOnce(&mut BackfillStatus)) -> BackfillStatus {
    let mut s = status_cell().lock().unwrap();
    f(&mut s);
    s.clone()
}

const MAX_RETRIES: u32 = 2;
const PROGRESS_EVERY: i64 = 500;

fn jitter() -> std::time::Duration {
    let mut bytes = [0u8; 2];
    let _ = getrandom::getrandom(&mut bytes);
    std::time::Duration::from_millis(500 + u64::from(u16::from_le_bytes(bytes)) % 1000)
}

async fn fetch_with_retries(source: &dyn StatisticsSource, limiter: &Limiter, slug: &str) -> Result<String, FetchError> {
    let mut retries = 0;
    loop {
        limiter.acquire(Lane::Hot).await;
        match source.fetch(slug).await {
            Ok(body) => return Ok(body),
            Err(FetchError::NotFound) => return Err(FetchError::NotFound),
            Err(error) => {
                if error == FetchError::RateLimited {
                    limiter.report_429();
                }
                if retries >= MAX_RETRIES {
                    return Err(error);
                }
                retries += 1;
                tokio::time::sleep(jitter()).await;
            }
        }
    }
}

/// Imports every item once, driving the process-wide status (spec §24 K3). `items` are `(item_id, slug)`.
pub async fn run(conn: &DatabaseConnection, source: &dyn StatisticsSource, limiter: &Limiter, items: Vec<(String, String)>) -> BackfillStatus {
    let total = items.len() as i64;
    update(|s| *s = BackfillStatus { state: BackfillState::Running, started_at: Some(ts(Utc::now())), items_total: total, ..BackfillStatus::default() });
    info("Backfill", &format!("Started: {} items", total), &LoggerOptions::default());
    for (item_id, slug) in items {
        match fetch_with_retries(source, limiter, &slug).await {
            Ok(body) => match parse_statistics(&body) {
                Ok(days) => match insert_missing(conn, &item_id, &days).await {
                    Ok(n) => {
                        update(|s| s.days_inserted += n as i64);
                    }
                    Err(e) => {
                        warning("Backfill", &format!("{slug}: insert failed: {}", e.message), &LoggerOptions::default());
                        update(|s| { s.items_failed += 1; s.last_error = Some(e.message.clone()); });
                    }
                },
                Err(e) => {
                    warning("Backfill", &format!("{slug}: {}", e.message), &LoggerOptions::default());
                    update(|s| { s.items_failed += 1; s.last_error = Some(e.message.clone()); });
                }
            },
            Err(FetchError::NotFound) => {
                update(|s| s.items_missing += 1);
            }
            Err(e) => {
                warning("Backfill", &format!("{slug}: {e}"), &LoggerOptions::default());
                update(|s| { s.items_failed += 1; s.last_error = Some(e.to_string()); });
            }
        }
        let s = update(|s| s.items_done += 1);
        if s.items_done % PROGRESS_EVERY == 0 {
            info("Backfill", &format!("Progress: {}/{} items, {} days inserted", s.items_done, s.items_total, s.days_inserted), &LoggerOptions::default());
        }
    }
    let final_status = update(|s| { s.state = BackfillState::Done; s.finished_at = Some(ts(Utc::now())); });
    info(
        "Backfill",
        &format!("Finished: items {}, days {}, missing {}, failed {}", final_status.items_done, final_status.days_inserted, final_status.items_missing, final_status.items_failed),
        &LoggerOptions::default(),
    );
    final_status
}

/// Starts a run in the background unless one is running; returns the status either way.
pub fn start(conn: DatabaseConnection, items: Vec<(String, String)>) -> BackfillStatus {
    {
        let mut s = status_cell().lock().unwrap();
        if s.state == BackfillState::Running {
            return s.clone();
        }
        s.state = BackfillState::Running;
    }
    tokio::spawn(async move {
        let source = HttpStatisticsSource::new(reqwest::Client::new(), "https://api.warframe.market/v1");
        run(&conn, &source, crate::market::limiter::global(), items).await;
    });
    status()
}
```

Check `utils::info`'s exact signature in the crate (the collector calls `info("Collector", ..., &LoggerOptions::default())` and item.rs calls `info(comp("..."), &msg, ...)`); match it. If `Limiter` has no test constructor, add the smallest `pub fn new(...)` the tests need next to `global()` (do not change `global()`'s behaviour).

- [ ] **Step 4: Run the backfill tests**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib collector::backfill`
Expected: 4 passed. The paused-time test does not wait for the jitter sleeps.

- [ ] **Step 5: Write the failing rpc test** in `rpc.rs` `tests`:

```rust
    #[tokio::test]
    async fn market_backfill_commands_are_routable() {
        for name in ["market_backfill_start", "market_backfill_status"] {
            assert!(COMMANDS.contains(&name), "{name}");
        }
    }
```

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib commands::rpc::tests::market_backfill`
Expected: FAIL.

- [ ] **Step 6: Commands** in `commands/market.rs`:

```rust
use crate::collector::backfill::{self, BackfillStatus};

/// Starts the 90-day statistics import unless it is already running (spec §24 K4).
pub async fn market_backfill_start() -> Result<BackfillStatus, Error> {
    let conn = database()?.clone();
    let items = states::cache_client()?
        .tradable_item()
        .get_items()?
        .into_iter()
        .map(|item| (item.wfm_id, item.wfm_url))
        .collect();
    Ok(backfill::start(conn, items))
}

pub async fn market_backfill_status() -> Result<BackfillStatus, Error> {
    Ok(backfill::status())
}
```

and the rows in `rpc.rs` after `market_warmup`:

```rust
    market_backfill_start => market::market_backfill_start {},
    market_backfill_status => market::market_backfill_status {},
```

- [ ] **Step 7: Run the suite and the script**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib && python3 scripts/check-rpc-commands.py`
Expected: all pass; the script's server count is 2 above "used by web", `0 missing`.

- [ ] **Step 8: Commit and push**

```bash
git add crates/qf_core/src/collector/backfill.rs crates/qf_core/src/commands/market.rs crates/qf_core/src/commands/rpc.rs crates/qf_core/src/market/limiter.rs
git commit -m "feat(collector): run the statistics backfill through the limiter with a process-wide status and two RPCs

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

(Only add `limiter.rs` if you touched it.)

---

### Task 3: Web — the button and status line on the Collector tab

**Files:**
- Modify: `web/src/api/market/index.ts`, `web/src/types/tauri.type.ts`, `web/src/pages/market_data/Tabs/Collector/index.tsx`, `web/public/lang/en.json`

- [ ] **Step 1: Types** after `MarketWarmup`:

```ts
  export type MarketBackfillState = "idle" | "running" | "done" | "failed";
  export interface MarketBackfillStatus {
    state: MarketBackfillState;
    started_at?: string | null;
    finished_at?: string | null;
    items_total: number;
    items_done: number;
    days_inserted: number;
    items_missing: number;
    items_failed: number;
    last_error?: string | null;
  }
```

- [ ] **Step 2: API** in `web/src/api/market/index.ts`:

```ts
  backfillStart() {
    return this.client.sendInvoke<TauriTypes.MarketBackfillStatus>("market_backfill_start");
  }
  backfillStatus() {
    return this.client.sendInvoke<TauriTypes.MarketBackfillStatus>("market_backfill_status");
  }
```

- [ ] **Step 3: Strings** — insert under `pages.market_data.tabs.collector` by targeted text edit (one block):

```json
"backfill": {
  "title": "Historical data",
  "button": "Import 90-day history from warframe.market",
  "idle": "Not imported yet. This fetches every item's closed-trade history once and fills only days the collector has not produced. About twenty minutes.",
  "running": "Importing… {{done}}/{{total}} items, {{days}} days added",
  "done": "Finished at {{at}}: {{items}} items, {{days}} days added, {{missing}} missing, {{failed}} failed",
  "failed": "Failed: {{error}}"
}
```

Confirm the file parses and `git diff --stat` shows one block.

- [ ] **Step 4: The block** at the bottom of `CollectorPanel`'s `<Stack>` (add `Button`, `Paper`, `Title` to the Mantine import; `useMutation`, `useQuery`, `useQueryClient` from TanStack; `dayjs` is already imported):

```tsx
      <Paper withBorder p="sm">
        <Title order={5} mb="xs">{t("backfill.title")}</Title>
        <BackfillControls t={t} />
      </Paper>
```

and the component in the same file, below `Stat`:

```tsx
function BackfillControls({ t }: { t: (key: string, context?: { [key: string]: any }) => string }) {
  const queryClient = useQueryClient();
  const { data } = useQuery({
    queryKey: ["market_backfill_status"],
    queryFn: () => api.market.backfillStatus(),
    refetchInterval: (query) => (query.state.data?.state === "running" ? 5_000 : false),
  });
  const start = useMutation({
    mutationFn: () => api.market.backfillStart(),
    onSuccess: (status) => queryClient.setQueryData(["market_backfill_status"], status),
  });
  const running = data?.state === "running" || start.isPending;
  const line =
    !data || data.state === "idle"
      ? t("backfill.idle")
      : data.state === "running"
        ? t("backfill.running", { done: data.items_done, total: data.items_total, days: data.days_inserted })
        : data.state === "done"
          ? t("backfill.done", { at: when(data.finished_at), items: data.items_done, days: data.days_inserted, missing: data.items_missing, failed: data.items_failed })
          : t("backfill.failed", { error: data.last_error ?? "" });
  return (
    <Group>
      <Button onClick={() => start.mutate()} disabled={running} loading={running}>
        {t("backfill.button")}
      </Button>
      <Text size="sm" c={data?.state === "failed" ? "red" : "dimmed"}>
        {line}
      </Text>
    </Group>
  );
}
```

`refetchInterval` as a function receives the query in TanStack Query 5; if the installed version's typing differs, check `node_modules/@tanstack/react-query/build/modern/index.d.ts` and match it.

- [ ] **Step 5: Build**

Run: `python3 scripts/check-rpc-commands.py && (cd web && pnpm build)`
Expected: server and web counts equal, `0 missing`; tsc and vite clean.

- [ ] **Step 6: Commit and push**

```bash
git add web/src/api/market/index.ts web/src/types/tauri.type.ts web/src/pages/market_data/Tabs/Collector/index.tsx web/public/lang/en.json
git commit -m "feat(web): add the 90-day history import button and status to the Collector tab

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 4: Deploy, run the import, record acceptance (needs the user)

- [ ] **Step 1:** The user runs the two `!` deploy commands (rsync with the `crates/*/logs` exclusion, then compose rebuild with the boot log); the controller reads the saved output for the boot markers.
- [ ] **Step 2: K7 checks** with the user: press the button; Log tab shows `[Backfill] Started: N items` and progress lines about every 500 items; button disabled with the status counting up; within about 25 minutes the status reads Finished with items ≈ 3 840, days in the hundreds of thousands, missing small, failed 0 or near it; Price History for Ash Prime Set shows about 90 daily bars; Movers 7 d lists populated; Warm-up unchanged (warm = 0 before 2026-09-22); pressing the button again finishes with 0 days added.
- [ ] **Step 3:** `docs/PHASE-6C-ACCEPTANCE.md` in the shape of `docs/PHASE-6B-ACCEPTANCE.md`; commit and push. The controller merges after the gate is green on the tip.

---

## Self-review

- **Spec coverage.** K1 → Task 1 (`parse_statistics`); K2 → Task 1 (`insert_missing`, `INSERT OR IGNORE`, nothing else written); K3 → Task 2 (`run`, status, retries, 404/failed, logs, single run); K4 → Tasks 2–3; K5 → nothing built; K6 → Tasks 1–2 tests, rpc test, web checks; K7 → Task 4.
- **Placeholders.** None; the two "check the installed API" notes name the exact file to read and the exact thing to match.
- **Type consistency.** `BackfillStatus` fields match `MarketBackfillStatus`; `BackfillState` serialises lowercase matching `MarketBackfillState`; `run` returns and `status()` snapshots the same struct; `start` is what `market_backfill_start` calls with `(wfm_id, wfm_url)` pairs.
