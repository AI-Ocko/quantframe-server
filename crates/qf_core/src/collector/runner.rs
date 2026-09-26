//! Supervised collector tasks: hot loop, cold loop, maintenance and daily item refresh.

use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration as StdDuration, Instant};

use chrono::{Duration, Utc};
use service::sea_orm::DatabaseConnection;
use utils::{error, info, warning, Error, LoggerOptions};

use super::backfill::{HttpStatisticsSource, WFM_API_V1};
use super::closed;
use super::fetch::{fetch_with_retries, FetchError, HttpOrderSource, OrderSource, WFM_API_V2};
use super::health::HealthTracker;
use super::maintenance;
use super::orders::V2Order;
use super::stats::StatsConfig;
use super::store::{self, SweepInput, SweepTarget};
use super::trade_tax;
use super::ts;
use crate::game_data;
use crate::market::limiter::{self, Lane, Limiter};
use crate::utils::modules::states;

pub const HOT_INTERVAL_S: i64 = 300;
const HOT_SET_REFRESH: StdDuration = StdDuration::from_secs(60);
const IDLE: StdDuration = StdDuration::from_secs(1);
const ERROR_PAUSE: StdDuration = StdDuration::from_secs(5);
/// Spec P23: one cold-lane item every 5 s (~0.2 req/s) instead of back to back. There is no
/// user-facing collector settings struct (`StatsConfig` is internal; `QF_COLLECTOR` is the only
/// env knob), so this is a constant rather than a configurable field.
pub const COLD_PACE_S: u64 = 5;
const COLD_PACE: StdDuration = StdDuration::from_secs(COLD_PACE_S);
const RESTART_DELAY: StdDuration = StdDuration::from_secs(5);
const RESOLVE_EVERY: StdDuration = StdDuration::from_secs(5 * 60);
const HOURLY_EVERY: StdDuration = StdDuration::from_secs(60 * 60);
const ITEM_REFRESH_EVERY: StdDuration = StdDuration::from_secs(24 * 60 * 60);
const COLD_CANDIDATES: i64 = 200;
const MIN_COLD_INTERVAL_S: i64 = 60;

pub struct Collector {
    pub conn: DatabaseConnection,
    pub cfg: StatsConfig,
    pub health: HealthTracker,
    source: Arc<dyn OrderSource>,
    limiter: &'static Limiter,
    hot: Mutex<HashSet<String>>,
    in_flight: Mutex<HashSet<String>>,
}

/// Keeps two loops from sweeping the same item at once.
struct InFlight<'a> {
    set: &'a Mutex<HashSet<String>>,
    id: String,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.set.lock().unwrap().remove(&self.id);
    }
}

fn lane_name(lane: Lane) -> &'static str {
    match lane {
        Lane::Trader => "trader",
        Lane::Hot => "hot",
        Lane::Cold => "cold",
    }
}

impl Collector {
    pub fn new(conn: DatabaseConnection, source: Arc<dyn OrderSource>, limiter: &'static Limiter, cfg: StatsConfig) -> Self {
        Self {
            conn,
            cfg,
            health: HealthTracker::default(),
            source,
            limiter,
            hot: Mutex::new(HashSet::new()),
            in_flight: Mutex::new(HashSet::new()),
        }
    }

    pub fn set_hot(&self, ids: HashSet<String>) {
        self.health.set_hot_items(ids.len());
        *self.hot.lock().unwrap() = ids;
    }

    pub fn hot_ids(&self) -> HashSet<String> {
        self.hot.lock().unwrap().clone()
    }

    /// Stock, wish list and, when Buy mode is on, the trader's buy candidates (amendments B3, C3).
    pub async fn refresh_hot_set(&self) -> Result<(), Error> {
        let mut ids = store::hot_item_ids(&self.conn).await?;
        if let Some(app) = states::try_app_state() {
            let cache = states::cache_client()?;
            let prices = crate::trader::price_source::StatsPriceSource::load(&self.conn, &cache).await?;
            ids.extend(crate::trader::price_source::buy_candidate_ids(&app.settings, &prices));
        }
        self.set_hot(ids);
        Ok(())
    }

    fn claim(&self, item_id: &str) -> Option<InFlight<'_>> {
        let mut set = self.in_flight.lock().unwrap();
        set.insert(item_id.to_string()).then(|| InFlight { set: &self.in_flight, id: item_id.to_string() })
    }

    /// Cold items are expected back after one full pass (amendment B7).
    pub async fn cold_expected_interval_s(&self) -> Result<i64, Error> {
        let estimate = match self.health.last_cold_pass_seconds() {
            Some(seconds) => seconds,
            None => store::active_count(&self.conn).await?,
        };
        Ok(estimate.max(MIN_COLD_INTERVAL_S))
    }

    /// Sweeps the most overdue hot item not attempted in the last 5 min. Returns its id.
    pub async fn hot_step(&self) -> Result<Option<String>, Error> {
        let due_before = ts(Utc::now() - Duration::seconds(HOT_INTERVAL_S));
        let targets = store::hot_targets(&self.conn, &self.hot_ids()).await?;
        for target in targets {
            if target.last_attempt_at.as_deref().is_some_and(|a| a > due_before.as_str()) {
                continue;
            }
            if let Some(_claim) = self.claim(&target.item_id) {
                self.sweep(&target, Lane::Hot, HOT_INTERVAL_S).await?;
                return Ok(Some(target.item_id));
            }
        }
        Ok(None)
    }

    /// Sweeps the least recently attempted item that isn't hot. Returns its id.
    pub async fn cold_step(&self) -> Result<Option<String>, Error> {
        let now = Utc::now();
        let hot = self.hot_ids();
        let candidates = store::cold_candidates(&self.conn, COLD_CANDIDATES).await?;
        for target in candidates {
            if hot.contains(&target.item_id) {
                continue;
            }
            let Some(_claim) = self.claim(&target.item_id) else { continue };
            let pass_started = ts(self.health.cold_pass_start_or_init(now));
            if target.last_attempt_at.as_deref().is_some_and(|a| a >= pass_started.as_str()) {
                self.health.finish_cold_pass(now);
            }
            let expected = self.cold_expected_interval_s().await?;
            self.sweep(&target, Lane::Cold, expected).await?;
            return Ok(Some(target.item_id));
        }
        Ok(None)
    }

    /// Records an order book the trader fetched live as a `trader` sweep (spec P23). Skips it
    /// when another loop is sweeping the item right now.
    pub async fn ingest_book(&self, item_id: &str, slug: &str, orders: &[V2Order], expected_interval_s: i64) -> Result<(), Error> {
        let Some(_claim) = self.claim(item_id) else { return Ok(()) };
        let target = SweepTarget { item_id: item_id.to_string(), slug: slug.to_string(), last_attempt_at: None };
        self.record_sweep(&target, Lane::Trader, Ok(orders.to_vec()), Utc::now(), expected_interval_s).await
    }

    async fn sweep(&self, target: &SweepTarget, lane: Lane, expected_interval_s: i64) -> Result<(), Error> {
        let started = Utc::now();
        store::mark_attempt(&self.conn, &target.item_id, started).await?;
        let result = fetch_with_retries(self.source.as_ref(), self.limiter, lane, &target.slug).await;
        self.record_sweep(target, lane, result, started, expected_interval_s).await
    }

    /// Writes one fetched order book, or its failure, to the sweep tables and the health counters.
    pub async fn record_sweep(
        &self,
        target: &SweepTarget,
        lane: Lane,
        result: Result<Vec<V2Order>, FetchError>,
        started: chrono::DateTime<Utc>,
        expected_interval_s: i64,
    ) -> Result<(), Error> {
        match result {
            Ok(orders) => {
                store::apply_sweep(
                    &self.conn,
                    SweepInput {
                        item_id: &target.item_id,
                        lane: lane_name(lane),
                        orders: &orders,
                        swept_at: started,
                        expected_interval_s,
                        gap_factor: self.cfg.gap_factor,
                    },
                )
                .await?;
                maintenance::recompute_item_stats(&self.conn, &target.item_id, Utc::now(), &self.cfg).await?;
                self.health.record(Utc::now(), lane, true, None);
            }
            Err(FetchError::NotFound) => {
                store::deactivate(&self.conn, &target.item_id).await?;
                self.health.record(
                    Utc::now(),
                    lane,
                    false,
                    Some(format!("{}: not found, inactive until the next item refresh", target.slug)),
                );
            }
            // spec P21: a block is no attempt. Putting the attempt time back makes the item due
            // again as soon as the breaker lets the next request through.
            Err(FetchError::Blocked | FetchError::RateLimited) => {
                store::restore_attempt(&self.conn, &target.item_id, target.last_attempt_at.as_deref()).await?;
            }
            Err(e) => {
                store::record_error(&self.conn, &target.item_id).await?;
                self.health.record(Utc::now(), lane, false, Some(format!("{}: {}", target.slug, e)));
            }
        }
        Ok(())
    }
}

fn log_error(component: &str, e: &Error) {
    error(component, format!("{} ({})", e.message, e.component), &LoggerOptions::default());
}

/// Runs a task and restarts it after a panic (spec §8). A task that returns normally is not restarted.
pub fn supervise<F, Fut>(name: &'static str, restart_delay: StdDuration, make: F) -> tokio::task::JoinHandle<()>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            match tokio::spawn(make()).await {
                Ok(()) => return,
                Err(e) if e.is_panic() => {
                    error(
                        name,
                        format!("Task panicked; restarting in {} s", restart_delay.as_secs()),
                        &LoggerOptions::default(),
                    );
                    tokio::time::sleep(restart_delay).await;
                }
                Err(_) => return,
            }
        }
    })
}

async fn hot_loop(collector: Arc<Collector>) {
    let mut last_refresh: Option<Instant> = None;
    loop {
        if last_refresh.is_none_or(|t| t.elapsed() >= HOT_SET_REFRESH) {
            if let Err(e) = collector.refresh_hot_set().await {
                log_error("Collector:HotSet", &e);
            }
            last_refresh = Some(Instant::now());
        }
        match collector.hot_step().await {
            Ok(Some(_)) => {}
            Ok(None) => tokio::time::sleep(IDLE).await,
            Err(e) => {
                log_error("Collector:Hot", &e);
                tokio::time::sleep(ERROR_PAUSE).await;
            }
        }
    }
}

/// How long `cold_loop` waits before its next `cold_step` (spec P23): `COLD_PACE` after a sweep,
/// `IDLE` when nothing was due, `ERROR_PAUSE` after a real (non-fetch) error.
fn cold_loop_pace(result: &Result<Option<String>, Error>) -> StdDuration {
    match result {
        Ok(Some(_)) => COLD_PACE,
        Ok(None) => IDLE,
        Err(_) => ERROR_PAUSE,
    }
}

async fn cold_loop(collector: Arc<Collector>) {
    loop {
        let result = collector.cold_step().await;
        if let Err(e) = &result {
            log_error("Collector:Cold", e);
        }
        tokio::time::sleep(cold_loop_pace(&result)).await;
    }
}

async fn maintenance_loop(collector: Arc<Collector>) {
    let mut last_hourly: Option<Instant> = None;
    loop {
        let now = Utc::now();
        if let Err(e) = maintenance::resolve_and_recompute(&collector.conn, now, &collector.cfg).await {
            log_error("Collector:Resolve", &e);
        }
        if last_hourly.is_none_or(|t| t.elapsed() >= HOURLY_EVERY) {
            match maintenance::hourly(&collector.conn, now).await {
                Ok(report) => info(
                    "Collector:Maintenance",
                    format!(
                        "Hourly rows {}, daily rows {}, deleted summaries {}, deleted vanished {}",
                        report.hourly_rows, report.daily_rows, report.deleted_summaries, report.deleted_vanished
                    ),
                    &LoggerOptions::default(),
                ),
                Err(e) => log_error("Collector:Maintenance", &e),
            }
            collector.health.maintenance_done(now);
            last_hourly = Some(Instant::now());
        }
        tokio::time::sleep(RESOLVE_EVERY).await;
    }
}

async fn item_refresh_loop(collector: Arc<Collector>, cache_dir: PathBuf, http: reqwest::Client) {
    loop {
        tokio::time::sleep(ITEM_REFRESH_EVERY).await;
        match game_data::load_items(&cache_dir, &http).await {
            Ok(items) => {
                let pairs: Vec<(String, String)> = items.iter().map(|i| (i.wfm_id.clone(), i.wfm_url.clone())).collect();
                match states::cache_client() {
                    Ok(cache) => cache.tradable_item().set_items(items),
                    Err(e) => log_error("Collector:ItemRefresh", &e),
                }
                if let Err(e) = store::sync_items(&collector.conn, &pairs).await {
                    log_error("Collector:ItemRefresh", &e);
                }
                collector.health.item_refresh_done(Utc::now());
            }
            Err(e) => log_error("Collector:ItemRefresh", &e),
        }
    }
}

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
                        Ok(s) => info(
                            "ClosedStats",
                            format!("Pass complete: ok {}, missing {}, failed {}", s.ok, s.missing, s.failed),
                            &LoggerOptions::default(),
                        ),
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

/// One item-detail fetch every `TAX_PACE_S` while any active item's trade tax is unknown (spec §25 P18).
async fn trade_tax_loop(collector: Arc<Collector>, http: reqwest::Client) {
    let source = trade_tax::HttpItemDetailSource::new(http, WFM_API_V2);
    loop {
        match trade_tax::fetch_once(&collector.conn, &source, collector.limiter, &collector.hot_ids(), Utc::now()).await {
            Ok(Some(_)) => tokio::time::sleep(StdDuration::from_secs(trade_tax::TAX_PACE_S)).await,
            Ok(None) => tokio::time::sleep(StdDuration::from_secs(trade_tax::TAX_IDLE_S)).await,
            Err(e) => {
                log_error("TradeTax", &e);
                tokio::time::sleep(ERROR_PAUSE).await;
            }
        }
    }
}

static COLLECTOR: OnceLock<Arc<Collector>> = OnceLock::new();

pub fn get() -> Option<Arc<Collector>> {
    COLLECTOR.get().cloned()
}

pub struct CollectorStart {
    pub conn: DatabaseConnection,
    pub cache_dir: PathBuf,
    pub enabled: bool,
}

/// Syncs `sweep_state` with the loaded item list and starts the supervised collector tasks.
pub async fn start(opts: CollectorStart) -> Result<(), Error> {
    if !opts.enabled {
        warning("Collector", "QF_COLLECTOR=off; market data collection is disabled", &LoggerOptions::default());
        return Ok(());
    }
    let http = crate::market::http_client(StdDuration::from_secs(30));
    let items = states::cache_client()?.tradable_item().get_items()?;
    let pairs: Vec<(String, String)> = items.iter().map(|i| (i.wfm_id.clone(), i.wfm_url.clone())).collect();
    store::sync_items(&opts.conn, &pairs).await?;

    let collector = Arc::new(Collector::new(
        opts.conn,
        Arc::new(HttpOrderSource::new(http.clone(), WFM_API_V2)),
        limiter::global(),
        StatsConfig::default(),
    ));
    collector.refresh_hot_set().await?;
    collector.health.mark_started(Utc::now());
    collector.health.item_refresh_done(Utc::now());
    let _ = COLLECTOR.set(collector.clone());

    let c = collector.clone();
    supervise("Collector:Hot", RESTART_DELAY, move || hot_loop(c.clone()));
    let c = collector.clone();
    supervise("Collector:Cold", RESTART_DELAY, move || cold_loop(c.clone()));
    let c = collector.clone();
    supervise("Collector:Maintenance", RESTART_DELAY, move || maintenance_loop(c.clone()));
    let (c, dir, item_http) = (collector.clone(), opts.cache_dir, http.clone());
    supervise("Collector:ItemRefresh", RESTART_DELAY, move || item_refresh_loop(c.clone(), dir.clone(), item_http.clone()));
    let (c, tax_http) = (collector.clone(), http.clone());
    supervise("Collector:TradeTax", RESTART_DELAY, move || trade_tax_loop(c.clone(), tax_http.clone()));
    let c = collector.clone();
    supervise("Collector:ClosedStats", RESTART_DELAY, move || closed_stats_loop(c.clone(), http.clone()));

    info(
        "Collector",
        format!("Started: {} items, {} hot", pairs.len(), collector.hot_ids().len()),
        &LoggerOptions::default(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::fetch::tests::ScriptedSource;
    use crate::collector::orders::parse_orders_response;
    use crate::collector::store::tests::setup;
    use service::sea_orm::ConnectionTrait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SMALL: &str = include_str!("../../tests/fixtures/orders_small.json");

    fn fast_limiter() -> &'static Limiter {
        Box::leak(Box::new(Limiter::new(1000)))
    }

    fn collector(conn: DatabaseConnection, responses: Vec<Result<Vec<V2Order>, FetchError>>) -> Collector {
        Collector::new(conn, Arc::new(ScriptedSource::new(responses)), fast_limiter(), StatsConfig::default())
    }

    fn ok() -> Result<Vec<V2Order>, FetchError> {
        Ok(parse_orders_response(SMALL).unwrap())
    }

    #[tokio::test]
    async fn hot_step_sweeps_due_hot_items_once_per_interval() {
        let (_dir, conn) = setup().await;
        let c = collector(conn, vec![ok(), ok()]);
        c.set_hot(HashSet::from(["item2".to_string()]));
        assert_eq!(c.hot_step().await.unwrap(), Some("item2".to_string()));
        assert_eq!(c.hot_step().await.unwrap(), None, "item2 was just swept");
        assert_eq!(c.health.lane_health(Lane::Hot, Utc::now()).swept_last_hour, 1);
    }

    #[tokio::test]
    async fn cold_step_skips_hot_items_and_measures_the_pass() {
        let (_dir, conn) = setup().await;
        store::sync_items(&conn, &[("item1".into(), "slug1".into()), ("item2".into(), "slug2".into()), ("item3".into(), "slug3".into())])
            .await
            .unwrap();
        let c = collector(conn, vec![ok(), ok(), ok()]);
        c.set_hot(HashSet::from(["item2".to_string()]));
        assert_eq!(c.cold_step().await.unwrap(), Some("item1".to_string()));
        assert_eq!(c.cold_step().await.unwrap(), Some("item3".to_string()));
        assert!(c.health.last_cold_pass_seconds().is_none());
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        assert_eq!(c.cold_step().await.unwrap(), Some("item1".to_string()));
        assert!(c.health.last_cold_pass_seconds().is_some(), "every item was attempted, so the pass finished");
    }

    #[tokio::test]
    async fn not_found_deactivates_and_transient_errors_are_counted() {
        let (_dir, conn) = setup().await;
        let c = collector(
            conn.clone(),
            vec![
                Err(FetchError::NotFound),
                Err(FetchError::Transient("a".into())),
                Err(FetchError::Transient("b".into())),
                Err(FetchError::Transient("c".into())),
            ],
        );
        assert_eq!(c.cold_step().await.unwrap(), Some("item1".to_string()));
        assert_eq!(store::active_count(&conn).await.unwrap(), 1);
        assert_eq!(c.cold_step().await.unwrap(), Some("item2".to_string()));
        let cold = c.health.lane_health(Lane::Cold, Utc::now());
        assert_eq!(cold.errors_last_hour, 2);
        assert_eq!(c.health.snapshot(Utc::now(), Default::default(), fast_limiter().snapshot()).last_error.as_deref(), Some("slug2: c"));
    }

    #[tokio::test]
    async fn blocked_sweep_does_not_count_an_item_error() {
        let (_dir, conn) = setup().await;
        for blocked in [FetchError::Blocked, FetchError::RateLimited] {
            let c = collector(conn.clone(), vec![Err(blocked)]);
            assert_eq!(c.cold_step().await.unwrap(), Some("item1".to_string()));
            let row = conn
                .query_one(crate::collector::stmt("SELECT consecutive_errors, last_attempt_at FROM sweep_state WHERE item_id = 'item1'", vec![]))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(row.try_get::<i64>("", "consecutive_errors").unwrap(), 0);
            assert_eq!(row.try_get::<Option<String>>("", "last_attempt_at").unwrap(), None, "item1 is due again once the breaker closes");
            assert_eq!(c.health.lane_health(Lane::Cold, Utc::now()).errors_last_hour, 0);
        }
    }

    // `cold_loop` itself is an infinite loop over a real (file-backed) sqlite connection;
    // driving it under `start_paused` time proved unreliable in practice (pausing time before
    // the spawned task's db queries ever complete stalls it indefinitely: swept stayed at 0
    // through 12 simulated seconds with the task neither finishing nor erroring). So the pacing
    // decision is pulled into the pure `cold_loop_pace` helper below, tested directly, plus an
    // integration-shaped test that drives the real `cold_step` and checks `cold_loop_pace` makes
    // the same one-item-at-a-time call `cold_loop` would.

    #[test]
    fn cold_loop_pace_paces_success_idles_on_none_and_backs_off_on_error() {
        assert_eq!(cold_loop_pace(&Ok(Some("item1".to_string()))), COLD_PACE, "paces after a sweep");
        assert_eq!(cold_loop_pace(&Ok(None)), IDLE, "idles when nothing was due");
        assert_eq!(cold_loop_pace(&Err(Error::new("Test", "boom", "test"))), ERROR_PAUSE, "backs off on a real error");
    }

    /// Spec P23: the cold lane sweeps one item per `COLD_PACE_S`, not back to back. Drives the
    /// real `cold_step` three times (3 items due) and checks that `cold_loop` — which sleeps
    /// `cold_loop_pace(&result)` after every step — would pace every single one of them, not
    /// just the first.
    #[tokio::test]
    async fn cold_step_results_pace_one_item_at_a_time() {
        let (_dir, conn) = setup().await;
        store::sync_items(&conn, &[("item1".into(), "slug1".into()), ("item2".into(), "slug2".into()), ("item3".into(), "slug3".into())])
            .await
            .unwrap();
        let c = collector(conn, vec![ok(), ok(), ok()]);
        for _ in 0..3 {
            let result = c.cold_step().await;
            assert_eq!(cold_loop_pace(&result), COLD_PACE, "cold_loop sleeps COLD_PACE after each successful sweep");
            assert!(result.unwrap().is_some());
        }
        assert_eq!(c.health.lane_health(Lane::Cold, Utc::now()).swept_last_hour, 3, "one item per sweep, three sweeps total");
    }

    /// Each row as one JSON array string, so whole tables compare with `assert_eq!`.
    async fn rows(conn: &DatabaseConnection, sql: &str) -> Vec<String> {
        conn.query_all(crate::collector::stmt(sql, vec![])).await.unwrap().iter().map(|r| r.try_get("", "r").unwrap()).collect()
    }

    const LIVE_SET: &str =
        "SELECT json_array(order_id, item_id, sub_type, side, platinum, quantity, user_id) AS r FROM last_seen_orders ORDER BY order_id";
    const SUMMARIES: &str = "SELECT json_array(item_id, sub_type, min_sell, max_buy, sell_count, buy_count, sell_ingame, buy_ingame,
                                               top_sells, top_buys) AS r FROM sweep_summary ORDER BY sub_type";
    const STATE: &str = "SELECT json_array(item_id, expected_interval_s, consecutive_errors, last_swept_at IS NOT NULL,
                                           last_attempt_at = last_swept_at) AS r FROM sweep_state ORDER BY item_id";

    #[tokio::test]
    async fn ingested_book_writes_the_same_rows_as_a_hot_sweep() {
        let (_hot_dir, hot_conn) = setup().await;
        let hot = collector(hot_conn.clone(), vec![ok()]);
        hot.set_hot(HashSet::from(["item1".to_string()]));
        assert_eq!(hot.hot_step().await.unwrap(), Some("item1".to_string()));

        let (_dir, conn) = setup().await;
        let c = collector(conn.clone(), vec![]);
        let orders = ok().unwrap();
        {
            let _busy = c.claim("item1").unwrap();
            c.ingest_book("item1", "slug1", &orders, HOT_INTERVAL_S).await.unwrap();
            assert!(rows(&conn, LIVE_SET).await.is_empty(), "a book for an item being swept is skipped");
        }
        c.ingest_book("item1", "slug1", &orders, HOT_INTERVAL_S).await.unwrap();

        for sql in [LIVE_SET, SUMMARIES, STATE] {
            assert_eq!(rows(&conn, sql).await, rows(&hot_conn, sql).await, "{sql}");
        }
        assert!(!rows(&conn, LIVE_SET).await.is_empty());
        assert_eq!(rows(&conn, "SELECT DISTINCT lane AS r FROM sweep_summary").await, vec!["trader".to_string()]);
        assert_eq!(c.health.lane_health(Lane::Trader, Utc::now()).swept_last_hour, 1);
        let snap = c.health.snapshot(Utc::now(), Default::default(), fast_limiter().snapshot());
        assert_eq!(snap.trader.swept_last_hour, 1);
        assert_eq!(snap.hot.swept_last_hour + snap.cold.swept_last_hour, 0);
    }

    #[tokio::test]
    async fn hot_step_skips_an_item_the_trader_just_ingested() {
        let (_dir, conn) = setup().await;
        let source = Arc::new(ScriptedSource::new(vec![ok()]));
        let c = Collector::new(conn, source.clone(), fast_limiter(), StatsConfig::default());
        c.set_hot(HashSet::from(["item1".to_string()]));
        c.ingest_book("item1", "slug1", &ok().unwrap(), 120).await.unwrap();
        assert_eq!(c.hot_step().await.unwrap(), None, "the trader's book is fresh");
        assert_eq!(source.calls.load(Ordering::SeqCst), 0, "no request for the ingested item");
    }

    #[tokio::test(start_paused = true)]
    async fn supervise_restarts_a_panicked_task() {
        let runs = Arc::new(AtomicUsize::new(0));
        let counter = runs.clone();
        let handle = supervise("Test:Supervise", std::time::Duration::from_secs(1), move || {
            let counter = counter.clone();
            async move {
                if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                    panic!("first run fails");
                }
            }
        });
        handle.await.unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }
}
