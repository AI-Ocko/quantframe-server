//! Trader run loop and error classification (upstream `client.rs`, amendment C6).

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use utils::{Error, LogLevel};

use super::orders::{TradeOrders, MAX_CONSECUTIVE_FAILURES};

pub const CYCLE_PAUSE: Duration = Duration::from_secs(1);
/// After a cycle that processed nothing (spec §21 G2). Slept in 1 s slices so Stop is not delayed.
pub const IDLE_PAUSE: Duration = Duration::from_secs(30);
static LOG_FILE: &str = "trader_item.log";

#[derive(Debug, Clone, PartialEq)]
pub enum EngineExit {
    Stopped,
    Critical(String),
    OrderFailures(u32),
}

/// Upstream classification: these wf-market error types stop the trader; everything else is a warning.
pub fn classify(error: &Error) -> LogLevel {
    match error.properties.get_property_value("type", String::new()).as_str() {
        "ParsingError" | "BadRequest" | "Unknown" | "InternalServerError" | "InvalidType" => LogLevel::Critical,
        _ => LogLevel::Warning,
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trader::orders::{Route, WriteMeta};
    use serde_json::json;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;
    use utils::Properties;
    use wf_market::enums::OrderType;
    use wf_market::types::{CreateOrderParams, SubType as WFSubType};

    fn wfm_error(kind: &str) -> Error {
        let mut e = Error::new("Test", "boom", "here");
        e.properties = Properties::from(json!({"type": kind}));
        e
    }

    #[test]
    fn critical_types_match_upstream() {
        for kind in ["ParsingError", "BadRequest", "Unknown", "InternalServerError", "InvalidType"] {
            assert!(matches!(classify(&wfm_error(kind)), LogLevel::Critical), "{kind}");
        }
        for kind in ["TooManyRequests", "NotFound", "OrderLimitExceeded", ""] {
            assert!(matches!(classify(&wfm_error(kind)), LogLevel::Warning), "{kind}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn warnings_continue_and_just_started_is_only_true_first() {
        let running = Arc::new(AtomicBool::new(true));
        let just_started = Arc::new(AtomicBool::new(false));
        let orders = Arc::new(TradeOrders::new(None, None, true));
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let check = {
            let (running, just_started, calls, seen) = (running.clone(), just_started.clone(), calls.clone(), seen.clone());
            move || {
                let (running, just_started, calls, seen) = (running.clone(), just_started.clone(), calls.clone(), seen.clone());
                async move {
                    seen.lock().unwrap().push(just_started.load(Ordering::SeqCst));
                    let n = calls.fetch_add(1, Ordering::SeqCst);
                    if n == 2 {
                        running.store(false, Ordering::SeqCst);
                    }
                    if n == 0 { Err(wfm_error("NotFound")) } else { Ok(1) }
                }
            }
        };
        let exit = run_loop(running, just_started, orders, check, CYCLE_PAUSE, IDLE_PAUSE).await;
        assert_eq!(exit, EngineExit::Stopped);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(*seen.lock().unwrap(), vec![true, false, false]);
    }

    #[tokio::test(start_paused = true)]
    async fn critical_error_stops_the_loop() {
        let running = Arc::new(AtomicBool::new(true));
        let orders = Arc::new(TradeOrders::new(None, None, true));
        let exit = run_loop(running.clone(), Arc::new(AtomicBool::new(false)), orders, || async { Err(wfm_error("BadRequest")) }, CYCLE_PAUSE, IDLE_PAUSE).await;
        assert_eq!(exit, EngineExit::Critical("Test: boom".into()));
        assert!(!running.load(Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn five_consecutive_order_failures_stop_the_loop() {
        let orders = Arc::new(TradeOrders::new(None, None, false));
        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            let params = CreateOrderParams::new_with_subtype("item1", OrderType::Buy, 10, 1, true, None, WFSubType::default());
            let _ = orders.create(params, Route::Live, &WriteMeta::default()).await;
        }
        let exit = run_loop(Arc::new(AtomicBool::new(true)), Arc::new(AtomicBool::new(false)), orders, || async { Ok(1) }, CYCLE_PAUSE, IDLE_PAUSE).await;
        assert_eq!(exit, EngineExit::OrderFailures(MAX_CONSECUTIVE_FAILURES));
    }

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
}
