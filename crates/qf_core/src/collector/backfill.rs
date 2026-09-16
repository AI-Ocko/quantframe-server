//! One-off import of warframe.market's 90-day closed-trade statistics (spec §24).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use service::sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};
use tokio::task::JoinHandle;
use utils::{get_location, info, warning, Error, LoggerOptions};

use super::fetch::{FetchError, MAX_RETRIES};
use super::orders::sub_type_key;
use super::{db_err, stmt, ts};
use crate::market::limiter::{Lane, Limiter};

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
    let txn = conn.begin().await.map_err(|e| db_err(C, e))?;
    let mut inserted = 0;
    for d in days {
        let result = txn
            .execute(stmt(
                "INSERT OR IGNORE INTO item_stats_daily (item_id, sub_type, day, volume, median, min_price, max_price) VALUES (?, ?, ?, ?, ?, ?, ?)",
                vec![item_id.into(), d.sub_type.clone().into(), d.day.clone().into(), d.volume.into(), d.median.into(), d.min_price.into(), d.max_price.into()],
            ))
            .await
            .map_err(|e| db_err(C, e))?;
        inserted += result.rows_affected();
    }
    txn.commit().await.map_err(|e| db_err(C, e))?;
    Ok(inserted)
}

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
            let response = self
                .http
                .get(&url)
                .header("Platform", "pc")
                .header("Language", "en")
                .send()
                .await
                .map_err(|e| FetchError::Transient(e.to_string()))?;
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

/// Locks the status, taking the value back out of a poisoned lock: a panicked run must not wedge
/// every later read and write.
fn status_lock() -> MutexGuard<'static, BackfillStatus> {
    STATUS.get_or_init(|| Mutex::new(BackfillStatus::default())).lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The process-wide status of the one backfill run (spec §24 K3).
pub fn status() -> BackfillStatus {
    status_lock().clone()
}

fn update(f: impl FnOnce(&mut BackfillStatus)) -> BackfillStatus {
    let mut status = status_lock();
    f(&mut status);
    status.clone()
}

const PROGRESS_EVERY: i64 = 500;

/// 500-1500 ms between retries, scaled away under `cfg(test)`: the sqlite pool cannot run on a
/// paused clock, so the tests take the real retry path and must not wait it out.
fn jitter() -> Duration {
    let mut bytes = [0u8; 2];
    let _ = getrandom::getrandom(&mut bytes);
    let millis = 500 + u64::from(u16::from_le_bytes(bytes)) % 1000;
    Duration::from_millis(if cfg!(test) { 0 } else { millis })
}

/// Takes a limiter token for every attempt and retries everything but a 404 at most twice with jitter.
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
    const C: &str = "Backfill";
    let total = items.len() as i64;
    update(|s| {
        *s = BackfillStatus { state: BackfillState::Running, started_at: Some(ts(Utc::now())), items_total: total, ..BackfillStatus::default() }
    });
    if items.is_empty() {
        const EMPTY: &str = "no tradable items in the cache";
        warning(C, EMPTY, &LoggerOptions::default());
        return update(|s| {
            s.state = BackfillState::Failed;
            s.finished_at = Some(ts(Utc::now()));
            s.last_error = Some(EMPTY.into());
        });
    }
    info(C, format!("Started: {total} items"), &LoggerOptions::default());
    for (item_id, slug) in items {
        let outcome = match fetch_with_retries(source, limiter, &slug).await {
            Ok(body) => match parse_statistics(&body) {
                Ok(days) => insert_missing(conn, &item_id, &days).await.map(Some),
                Err(e) => Err(e),
            },
            Err(FetchError::NotFound) => Ok(None),
            Err(e) => Err(Error::new(C, e.to_string(), get_location!())),
        };
        match outcome {
            Ok(Some(days)) => {
                update(|s| s.days_inserted += days as i64);
            }
            Ok(None) => {
                update(|s| s.items_missing += 1);
            }
            Err(e) => {
                warning(C, format!("{slug}: {}", e.message), &LoggerOptions::default());
                update(|s| {
                    s.items_failed += 1;
                    s.last_error = Some(e.message);
                });
            }
        }
        let status = update(|s| s.items_done += 1);
        if status.items_done % PROGRESS_EVERY == 0 {
            info(
                C,
                format!("Progress: {}/{} items, {} days inserted", status.items_done, status.items_total, status.days_inserted),
                &LoggerOptions::default(),
            );
        }
    }
    let final_status = update(|s| {
        s.state = BackfillState::Done;
        s.finished_at = Some(ts(Utc::now()));
    });
    info(
        C,
        format!(
            "Finished: items {}, days {}, missing {}, failed {}",
            final_status.items_done, final_status.days_inserted, final_status.items_missing, final_status.items_failed
        ),
        &LoggerOptions::default(),
    );
    final_status
}

/// Starts a run in the background unless one is already running; returns the status either way (spec §24 K4).
pub fn start(conn: DatabaseConnection, items: Vec<(String, String)>) -> BackfillStatus {
    let fresh = {
        let mut status = status_lock();
        if status.state == BackfillState::Running {
            return status.clone();
        }
        *status = BackfillStatus {
            state: BackfillState::Running,
            started_at: Some(ts(Utc::now())),
            items_total: items.len() as i64,
            ..BackfillStatus::default()
        };
        status.clone()
    };
    tokio::spawn(watch(tokio::spawn(async move {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().unwrap_or_default();
        let source = HttpStatisticsSource::new(http, "https://api.warframe.market/v1");
        run(&conn, &source, crate::market::limiter::global(), items).await
    })));
    fresh
}

/// Produces the `failed` state (spec §24 K3): a panicked or cancelled run must not leave the status
/// `Running`, which would refuse every later start until the process restarts.
async fn watch(handle: JoinHandle<BackfillStatus>) {
    if let Err(join_error) = handle.await {
        warning("Backfill", format!("Run did not finish: {join_error}"), &LoggerOptions::default());
        update(|s| {
            s.state = BackfillState::Failed;
            s.finished_at = Some(ts(Utc::now()));
            s.last_error = Some(join_error.to_string());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::store::{exec, tests::setup};

    const SMALL: &str = include_str!("../../tests/fixtures/statistics_small.json");

    /// The status is process-wide, so the tests that write it run one at a time.
    fn status_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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

    struct Scripted(std::sync::Mutex<std::collections::HashMap<String, std::collections::VecDeque<Result<String, FetchError>>>>);
    impl StatisticsSource for Scripted {
        fn fetch<'a>(&'a self, slug: &'a str) -> StatsFuture<'a> {
            let next = self.0.lock().unwrap().get_mut(slug).and_then(|q| q.pop_front()).unwrap_or(Err(FetchError::Transient("unscripted".into())));
            Box::pin(async move { next })
        }
    }

    #[tokio::test]
    async fn run_counts_inserted_missing_and_failed_items_and_reports_progress() {
        let _guard = status_guard();
        let (_dir, conn) = setup().await;

        let scripted = Scripted(std::sync::Mutex::new(std::collections::HashMap::from([
            ("ok".to_string(), std::collections::VecDeque::from([Ok(SMALL.to_string())])),
            ("gone".to_string(), std::collections::VecDeque::from([Err(FetchError::NotFound)])),
            ("flaky".to_string(), std::collections::VecDeque::from([Err(FetchError::Transient("boom".into())), Ok(SMALL.to_string())])),
            ("dead".to_string(), std::collections::VecDeque::from([Err(FetchError::Transient("1".into())), Err(FetchError::Transient("2".into())), Err(FetchError::Transient("3".into()))])),
        ])));
        let limiter = Limiter::new(1000);
        let items = vec![
            ("id-ok".to_string(), "ok".to_string()),
            ("id-gone".to_string(), "gone".to_string()),
            ("id-flaky".to_string(), "flaky".to_string()),
            ("id-dead".to_string(), "dead".to_string()),
        ];
        let final_status = run(&conn, &scripted, &limiter, items).await;
        assert_eq!(final_status.state, BackfillState::Done);
        assert_eq!((final_status.items_total, final_status.items_done), (4, 4));
        assert_eq!(final_status.days_inserted, 8, "4 rows for ok + 4 for flaky");
        assert_eq!((final_status.items_missing, final_status.items_failed), (1, 1));
        assert!(final_status.last_error.is_some(), "the dead item's last transient error is kept");
        assert!(final_status.started_at.is_some() && final_status.finished_at.is_some());
        assert_eq!(status(), final_status, "the process-wide status holds the final snapshot");
    }

    #[tokio::test]
    async fn a_panicked_run_leaves_the_status_failed_instead_of_running() {
        let _guard = status_guard();
        update(|s| *s = BackfillStatus { state: BackfillState::Running, ..BackfillStatus::default() });
        watch(tokio::spawn(async move { panic!("boom") })).await;
        let status = status();
        assert_eq!(status.state, BackfillState::Failed, "a later start must not be refused forever");
        assert!(status.finished_at.is_some());
        assert!(status.last_error.is_some_and(|error| !error.is_empty()));
        update(|s| *s = BackfillStatus::default());
    }

    #[tokio::test]
    async fn start_refuses_a_second_run_and_leaves_its_status_alone() {
        let _guard = status_guard();
        let (_dir, conn) = setup().await;
        update(|s| *s = BackfillStatus { state: BackfillState::Running, items_total: 42, items_done: 7, ..BackfillStatus::default() });
        let status = start(conn, vec![]);
        assert_eq!(status.state, BackfillState::Running);
        assert_eq!((status.items_total, status.items_done), (42, 7), "the running job's status is untouched");
        update(|s| *s = BackfillStatus::default());
    }

    #[tokio::test]
    async fn an_empty_item_list_fails_instead_of_reporting_a_done_run() {
        let _guard = status_guard();
        let (_dir, conn) = setup().await;
        let scripted = Scripted(std::sync::Mutex::new(std::collections::HashMap::new()));
        let status = run(&conn, &scripted, &Limiter::new(1000), vec![]).await;
        assert_eq!(status.state, BackfillState::Failed);
        assert_eq!(status.last_error.as_deref(), Some("no tradable items in the cache"));
        assert!(status.finished_at.is_some());
        update(|s| *s = BackfillStatus::default());
    }
}
