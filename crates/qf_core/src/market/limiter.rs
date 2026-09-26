//! One shared warframe.market request budget with strict priority lanes (spec §5.3).

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::Notify;
use tokio::time::Instant;
use utils::{info, warning, LoggerOptions};

/// `live_scraper.general.market_requests_per_second` (spec P22): the default and its clamp range.
pub const DEFAULT_RATE_PER_SECOND: f64 = 2.5;
const RATE_RANGE: std::ops::RangeInclusive<f64> = 0.2..=2.9;
/// Breaker periods (spec P21); the last one repeats.
const BREAKER_STEPS: [Duration; 3] =
    [Duration::from_secs(15 * 60), Duration::from_secs(60 * 60), Duration::from_secs(4 * 60 * 60)];
const TRIP_AFTER_TRANSPORT_ERRORS: u32 = 5;
/// A probe that reports nothing within this long counts as failed.
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
/// The ladder starts again at 15 min after this long without a trip.
const LADDER_RESET_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const LOG: &str = "Market:Breaker";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    Trader = 0,
    Hot = 1,
    Cold = 2,
}

impl Lane {
    fn index(self) -> usize {
        self as usize
    }
}

/// What a warframe.market response (or the lack of one) tells the breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Challenge,
    RateLimited,
    TransportError,
}

/// 429 → RateLimited; ≥ 400 carrying `cf-mitigated: challenge`, or a 403/503 with an HTML body →
/// Challenge; any other ≥ 500 → TransportError (Cloudflare's 502/520–526 origin-error pages are
/// HTML too, and one of those must not open the breaker); everything else (any other 4xx) → Ok.
pub fn outcome_of(status: u16, headers: &reqwest::header::HeaderMap) -> Outcome {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("");
    if status == 429 {
        Outcome::RateLimited
    } else if (status >= 400 && header("cf-mitigated").eq_ignore_ascii_case("challenge"))
        || (matches!(status, 403 | 503) && header("content-type").to_ascii_lowercase().starts_with("text/html"))
    {
        Outcome::Challenge
    } else if status >= 500 {
        Outcome::TransportError
    } else {
        Outcome::Ok
    }
}

/// `try_acquire` refused: the breaker is open (or its probe is out) until `until`.
#[derive(Debug, Clone, PartialEq)]
pub struct BreakerOpen {
    pub until: DateTime<Utc>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BreakerSnapshot {
    /// closed | open | probing
    pub state: String,
    pub reason: Option<String>,
    pub opened_at: Option<String>,
    pub until: Option<String>,
    /// Ladder index of the current or most recent period (0 = 15 min).
    pub step: u32,
    pub trips_total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LimiterSnapshot {
    pub granted_trader: u64,
    pub granted_hot: u64,
    pub granted_cold: u64,
    pub waiting: usize,
    pub paused_ms: u64,
    pub rate_limited_total: u64,
    pub rate_per_second: f64,
    pub breaker: BreakerSnapshot,
}

struct Open {
    reason: String,
    opened_at: DateTime<Utc>,
    until: Instant,
    step: u32,
    /// When the probe was granted; `None` until the period ends.
    probe: Option<Instant>,
}

struct State {
    rate_per_second: f64,
    interval: Duration,
    /// The next slot is `last_grant + interval`, so a rate change applies to it at once.
    last_grant: Option<Instant>,
    waiting: [usize; 3],
    granted: [u64; 3],
    rate_limited_total: u64,
    breaker: Option<Open>,
    transport_errors_in_row: u32,
    /// When the breaker last tripped and at which step, for the ladder.
    last_trip_at: Option<(Instant, u32)>,
    trips_total: u64,
}

/// What the breaker lets a request do right now.
enum Pass {
    Yes,
    /// Granting this request makes it the period's probe.
    Probe,
    Blocked { until: Instant, reason: String },
}

impl State {
    /// Opens the breaker, or reopens it one step higher when it is already open (a failed probe).
    fn trip(&mut self, now: Instant, reason: String) {
        let step = match (&self.breaker, self.last_trip_at) {
            (Some(open), _) => open.step + 1,
            (None, Some((at, step))) if now < at + LADDER_RESET_AFTER => step + 1,
            _ => 0,
        };
        let period = BREAKER_STEPS[(step as usize).min(BREAKER_STEPS.len() - 1)];
        let opened_at = self.breaker.as_ref().map_or_else(Utc::now, |open| open.opened_at);
        warning(
            LOG,
            format!(
                "Market breaker open: {reason}; all warframe.market traffic paused until {}",
                to_utc(now + period, now).format("%H:%M UTC")
            ),
            &LoggerOptions::default(),
        );
        self.breaker = Some(Open { reason, opened_at, until: now + period, step, probe: None });
        self.last_trip_at = Some((now, step));
        self.transport_errors_in_row = 0;
        self.trips_total += 1;
    }

    /// A probe that has not reported within `PROBE_TIMEOUT` counts as failed.
    fn expire_lost_probe(&mut self, now: Instant) {
        if let Some(Open { probe: Some(at), .. }) = self.breaker {
            if now >= at + PROBE_TIMEOUT {
                self.trip(now, "the probe got no response within 60 s".into());
            }
        }
    }

    fn pass(&mut self, now: Instant) -> Pass {
        self.expire_lost_probe(now);
        match &self.breaker {
            None => Pass::Yes,
            Some(open) => match open.probe {
                None if now >= open.until => Pass::Probe,
                None => Pass::Blocked { until: open.until, reason: open.reason.clone() },
                Some(at) => Pass::Blocked { until: at + PROBE_TIMEOUT, reason: open.reason.clone() },
            },
        }
    }
}

fn to_utc(at: Instant, now: Instant) -> DateTime<Utc> {
    Utc::now() + at.saturating_duration_since(now)
}

pub struct Limiter {
    state: Mutex<State>,
    notify: Notify,
}

static GLOBAL: OnceLock<Limiter> = OnceLock::new();

/// The process-wide limiter every warframe.market REST call goes through.
pub fn global() -> &'static Limiter {
    GLOBAL.get_or_init(|| Limiter::new(DEFAULT_RATE_PER_SECOND))
}

/// Keeps a lane's waiting count right when an `acquire` future is dropped before it gets a slot.
struct Waiting<'a> {
    limiter: &'a Limiter,
    lane: Lane,
    granted: bool,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        if !self.granted {
            self.limiter.state.lock().unwrap().waiting[self.lane.index()] -= 1;
            self.limiter.notify.notify_waiters();
        }
    }
}

impl Limiter {
    /// Unclamped, so tests can run fast; `set_rate_per_second` is the clamped setting.
    pub fn new(rate_per_second: impl Into<f64>) -> Self {
        let rate_per_second = rate_per_second.into();
        Self {
            state: Mutex::new(State {
                rate_per_second,
                interval: Duration::from_secs_f64(1.0 / rate_per_second),
                last_grant: None,
                waiting: [0; 3],
                granted: [0; 3],
                rate_limited_total: 0,
                breaker: None,
                transport_errors_in_row: 0,
                last_trip_at: None,
                trips_total: 0,
            }),
            notify: Notify::new(),
        }
    }

    /// Waits for a request slot, through an open breaker if need be. A lane is served only
    /// while no higher lane is waiting.
    pub async fn acquire(&self, lane: Lane) {
        let _ = self.wait_for_slot(lane, false).await;
    }

    /// Like `acquire`, but fails at once while the breaker is open or its probe is out.
    pub async fn try_acquire(&self, lane: Lane) -> Result<(), BreakerOpen> {
        self.wait_for_slot(lane, true).await
    }

    async fn wait_for_slot(&self, lane: Lane, fail_fast: bool) -> Result<(), BreakerOpen> {
        self.state.lock().unwrap().waiting[lane.index()] += 1;
        let mut waiting = Waiting { limiter: self, lane, granted: false };
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let wake_at = {
                let mut s = self.state.lock().unwrap();
                let now = Instant::now();
                let pass = s.pass(now);
                if let (true, Pass::Blocked { until, reason }) = (fail_fast, &pass) {
                    return Err(BreakerOpen { until: to_utc(*until, now), reason: reason.clone() });
                }
                if s.waiting[..lane.index()].iter().any(|&n| n > 0) {
                    None
                } else {
                    let next_slot = s.last_grant.map_or(now, |at| at + s.interval);
                    let ready = match pass {
                        Pass::Blocked { until, .. } => until.max(next_slot),
                        _ => next_slot,
                    };
                    if ready <= now {
                        if let (Pass::Probe, Some(open)) = (pass, s.breaker.as_mut()) {
                            open.probe = Some(now);
                            info(LOG, "Market breaker probing", &LoggerOptions::default());
                        }
                        s.last_grant = Some(now);
                        s.waiting[lane.index()] -= 1;
                        s.granted[lane.index()] += 1;
                        waiting.granted = true;
                        drop(s);
                        self.notify.notify_waiters();
                        return Ok(());
                    }
                    Some(ready)
                }
            };
            match wake_at {
                None => notified.await,
                Some(at) => {
                    tokio::select! {
                        _ = tokio::time::sleep_until(at) => {}
                        _ = &mut notified => {}
                    }
                }
            }
        }
    }

    /// Every warframe.market request reports its outcome once. A block (challenge or 429)
    /// opens the breaker, or reopens it a step higher when it answers the probe; a normal
    /// response closes a probing breaker; 5 transport errors in a row trip it. Reports that
    /// arrive while the breaker is open and no probe is out are stragglers and change nothing.
    pub fn report(&self, outcome: Outcome) {
        {
            let mut s = self.state.lock().unwrap();
            let now = Instant::now();
            s.expire_lost_probe(now);
            if outcome == Outcome::RateLimited {
                s.rate_limited_total += 1;
            }
            let probing = s.breaker.as_ref().is_some_and(|open| open.probe.is_some());
            if s.breaker.is_some() && !probing {
                return;
            }
            match outcome {
                Outcome::Ok => {
                    s.transport_errors_in_row = 0;
                    if let Some(open) = s.breaker.take() {
                        let minutes = (Utc::now() - open.opened_at).num_minutes();
                        info(LOG, format!("Market breaker closed after {minutes} min"), &LoggerOptions::default());
                    }
                }
                Outcome::Challenge => s.trip(now, "Cloudflare challenge".into()),
                Outcome::RateLimited => s.trip(now, "HTTP 429 rate limited".into()),
                Outcome::TransportError if probing => s.trip(now, "the probe failed with a transport error".into()),
                Outcome::TransportError => {
                    s.transport_errors_in_row += 1;
                    if s.transport_errors_in_row >= TRIP_AFTER_TRANSPORT_ERRORS {
                        s.trip(now, format!("{TRIP_AFTER_TRANSPORT_ERRORS} transport errors in a row"));
                    }
                }
            }
        }
        self.notify.notify_waiters();
    }

    /// Clamped to 0.2..=2.9; waiters recompute their slot at once.
    pub fn set_rate_per_second(&self, rate: f64) {
        {
            let mut s = self.state.lock().unwrap();
            s.rate_per_second = rate.clamp(*RATE_RANGE.start(), *RATE_RANGE.end());
            s.interval = Duration::from_secs_f64(1.0 / s.rate_per_second);
        }
        self.notify.notify_waiters();
    }

    /// True while the breaker is open or probing; the same state `snapshot().breaker.state` reports.
    pub fn is_blocked(&self) -> bool {
        let mut s = self.state.lock().unwrap();
        s.expire_lost_probe(Instant::now());
        s.breaker.is_some()
    }

    pub fn snapshot(&self) -> LimiterSnapshot {
        let mut s = self.state.lock().unwrap();
        let now = Instant::now();
        s.expire_lost_probe(now);
        let open = s.breaker.as_ref();
        let breaker = BreakerSnapshot {
            state: match open {
                None => "closed",
                Some(Open { probe: None, .. }) => "open",
                Some(_) => "probing",
            }
            .into(),
            reason: open.map(|o| o.reason.clone()),
            opened_at: open.map(|o| o.opened_at.to_rfc3339()),
            until: open.map(|o| to_utc(o.until, now).to_rfc3339()),
            step: s.last_trip_at.map_or(0, |(_, step)| step),
            trips_total: s.trips_total,
        };
        LimiterSnapshot {
            granted_trader: s.granted[Lane::Trader.index()],
            granted_hot: s.granted[Lane::Hot.index()],
            granted_cold: s.granted[Lane::Cold.index()],
            waiting: s.waiting.iter().sum(),
            paused_ms: open.map_or(0, |o| o.until.saturating_duration_since(now).as_millis() as u64),
            rate_limited_total: s.rate_limited_total,
            rate_per_second: s.rate_per_second,
            breaker,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test(start_paused = true)]
    async fn slots_are_spaced_by_the_rate() {
        let limiter = Limiter::new(3);
        let start = Instant::now();
        let mut granted_at = Vec::new();
        for _ in 0..3 {
            limiter.acquire(Lane::Cold).await;
            granted_at.push(start.elapsed().as_millis());
        }
        // tokio's timer has millisecond resolution and rounds deadlines up.
        assert_eq!(granted_at[0], 0);
        assert!((333..=334).contains(&granted_at[1]), "{granted_at:?}");
        assert!((666..=668).contains(&granted_at[2]), "{granted_at:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn spacing_follows_set_rate() {
        let limiter = Limiter::new(3);
        limiter.set_rate_per_second(2.5);
        let start = Instant::now();
        let mut granted_at = Vec::new();
        for _ in 0..3 {
            limiter.acquire(Lane::Cold).await;
            granted_at.push(start.elapsed().as_millis());
        }
        assert_eq!(granted_at, vec![0, 400, 800]);
        assert_eq!(limiter.snapshot().rate_per_second, 2.5);
    }

    #[tokio::test(start_paused = true)]
    async fn a_live_rate_change_applies_to_the_next_slot() {
        let limiter = Arc::new(Limiter::new(2));
        limiter.acquire(Lane::Cold).await;
        let start = Instant::now();
        let waiter = {
            let limiter = limiter.clone();
            tokio::spawn(async move { limiter.acquire(Lane::Cold).await })
        };
        tokio::task::yield_now().await;
        limiter.set_rate_per_second(0.5); // the waiter was due at 500 ms; now 2 s after the last grant
        waiter.await.unwrap();
        assert_eq!(start.elapsed().as_millis(), 2000);
        limiter.set_rate_per_second(2.0);
        let t = Instant::now();
        limiter.acquire(Lane::Cold).await;
        assert_eq!(t.elapsed().as_millis(), 500);
    }

    #[test]
    fn rate_is_clamped() {
        let limiter = Limiter::new(3);
        limiter.set_rate_per_second(3.0);
        assert_eq!(limiter.snapshot().rate_per_second, 2.9);
        limiter.set_rate_per_second(0.0);
        assert_eq!(limiter.snapshot().rate_per_second, 0.2);
        limiter.set_rate_per_second(1.7);
        assert_eq!(limiter.snapshot().rate_per_second, 1.7);
        assert_eq!(global().snapshot().rate_per_second, DEFAULT_RATE_PER_SECOND);
    }

    #[tokio::test(start_paused = true)]
    async fn higher_lane_is_served_before_a_lower_lane_that_waited_longer() {
        let limiter = Arc::new(Limiter::new(3));
        limiter.acquire(Lane::Cold).await;
        let order = Arc::new(Mutex::new(Vec::new()));
        let spawn = |lane: Lane| {
            let (limiter, order) = (limiter.clone(), order.clone());
            tokio::spawn(async move {
                limiter.acquire(lane).await;
                order.lock().unwrap().push(lane);
            })
        };
        let cold = spawn(Lane::Cold);
        tokio::task::yield_now().await;
        let hot = spawn(Lane::Hot);
        tokio::task::yield_now().await;
        let trader = spawn(Lane::Trader);
        for task in [cold, hot, trader] {
            task.await.unwrap();
        }
        assert_eq!(*order.lock().unwrap(), vec![Lane::Trader, Lane::Hot, Lane::Cold]);
    }

    use reqwest::header::{HeaderMap, HeaderValue};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const MIN: u64 = 60;

    #[tokio::test(start_paused = true)]
    async fn challenge_opens_for_15_min_and_probe_success_closes() {
        let limiter = Limiter::new(3);
        limiter.report(Outcome::Challenge);
        let b = limiter.snapshot().breaker;
        assert_eq!(b.state, "open");
        assert!(limiter.is_blocked());
        assert!(b.reason.is_some() && b.opened_at.is_some() && b.until.is_some());
        let t = Instant::now();
        limiter.acquire(Lane::Cold).await;
        assert_eq!(t.elapsed().as_secs(), 15 * MIN);
        assert_eq!(limiter.snapshot().breaker.state, "probing");
        assert!(limiter.is_blocked());
        limiter.report(Outcome::Ok);
        let b = limiter.snapshot().breaker;
        assert_eq!((b.state.as_str(), b.until, b.trips_total), ("closed", None, 1));
        assert!(!limiter.is_blocked());
        let t = Instant::now();
        limiter.acquire(Lane::Cold).await;
        assert!(t.elapsed() <= Duration::from_millis(334));
    }

    #[tokio::test(start_paused = true)]
    async fn failed_probes_climb_15m_1h_4h_4h() {
        let limiter = Limiter::new(3);
        limiter.report(Outcome::Challenge);
        for (step, minutes) in [15, 60, 240, 240].into_iter().enumerate() {
            assert_eq!(limiter.snapshot().breaker.step, step as u32);
            let t = Instant::now();
            limiter.acquire(Lane::Trader).await;
            assert_eq!(t.elapsed().as_secs(), minutes * MIN);
            limiter.report(Outcome::Challenge);
        }
        assert_eq!(limiter.snapshot().breaker.trips_total, 5);
    }

    #[tokio::test(start_paused = true)]
    async fn ladder_resets_after_24h_closed() {
        let limiter = Limiter::new(3);
        for minutes in [15, 60] {
            limiter.report(Outcome::Challenge);
            let t = Instant::now();
            limiter.acquire(Lane::Hot).await;
            assert_eq!(t.elapsed().as_secs(), minutes * MIN);
            limiter.report(Outcome::Ok);
        }
        tokio::time::advance(Duration::from_secs(24 * 60 * MIN)).await;
        limiter.report(Outcome::Challenge);
        let t = Instant::now();
        limiter.acquire(Lane::Hot).await;
        assert_eq!(t.elapsed().as_secs(), 15 * MIN);
    }

    #[tokio::test(start_paused = true)]
    async fn only_one_probe_per_period() {
        let limiter = Arc::new(Limiter::new(3));
        let granted = Arc::new(AtomicUsize::new(0));
        limiter.report(Outcome::Challenge);
        for lane in [Lane::Hot, Lane::Cold] {
            let (limiter, granted) = (limiter.clone(), granted.clone());
            tokio::spawn(async move {
                limiter.acquire(lane).await;
                granted.fetch_add(1, Ordering::SeqCst);
            });
        }
        tokio::time::sleep(Duration::from_secs(15 * MIN + 1)).await;
        assert_eq!(granted.load(Ordering::SeqCst), 1);
        tokio::time::sleep(Duration::from_secs(50)).await;
        assert_eq!(granted.load(Ordering::SeqCst), 1, "second waiter got through before the probe reported");
        limiter.report(Outcome::Ok);
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(granted.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn lost_probe_counts_failed_after_60s() {
        let limiter = Limiter::new(3);
        limiter.report(Outcome::Challenge);
        limiter.acquire(Lane::Trader).await; // the probe, never reported
        let t = Instant::now();
        limiter.acquire(Lane::Trader).await;
        assert_eq!(t.elapsed().as_secs(), 60 + 60 * MIN);
        let b = limiter.snapshot().breaker;
        assert_eq!((b.state.as_str(), b.step, b.trips_total), ("probing", 1, 2));
    }

    #[tokio::test(start_paused = true)]
    async fn five_transport_errors_trip_and_a_response_resets_the_count() {
        let limiter = Limiter::new(3);
        for _ in 0..4 {
            limiter.report(Outcome::TransportError);
        }
        limiter.report(Outcome::Ok);
        for _ in 0..4 {
            limiter.report(Outcome::TransportError);
        }
        assert_eq!(limiter.snapshot().breaker.state, "closed");
        limiter.report(Outcome::TransportError);
        let b = limiter.snapshot().breaker;
        assert_eq!(b.state, "open");
        assert!(b.reason.unwrap().contains("transport"));
    }

    #[tokio::test(start_paused = true)]
    async fn try_acquire_fails_fast_while_open() {
        let limiter = Limiter::new(3);
        limiter.report(Outcome::Challenge);
        let t = Instant::now();
        let open = limiter.try_acquire(Lane::Trader).await.unwrap_err();
        assert_eq!(t.elapsed(), Duration::ZERO);
        let left = (open.until - Utc::now()).num_seconds();
        assert!((15 * MIN as i64 - 5..=15 * MIN as i64).contains(&left), "{left}");
        assert!(!open.reason.is_empty());
        assert_eq!(limiter.snapshot().waiting, 0);

        tokio::time::sleep(Duration::from_secs(15 * MIN)).await;
        limiter.try_acquire(Lane::Trader).await.unwrap(); // the probe
        assert!(limiter.try_acquire(Lane::Trader).await.is_err(), "a second request went out while probing");
        limiter.report(Outcome::Ok);
        limiter.try_acquire(Lane::Trader).await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_429_trips() {
        let limiter = Limiter::new(3);
        limiter.report(Outcome::RateLimited);
        let snapshot = limiter.snapshot();
        assert_eq!(snapshot.breaker.state, "open");
        assert_eq!(snapshot.rate_limited_total, 1);
        assert_eq!(snapshot.paused_ms, 15 * MIN * 1000);
        let t = Instant::now();
        limiter.acquire(Lane::Cold).await;
        assert_eq!(t.elapsed().as_secs(), 15 * MIN);
    }

    #[tokio::test(start_paused = true)]
    async fn open_blocks_all_lanes() {
        let limiter = Arc::new(Limiter::new(3));
        limiter.report(Outcome::Challenge);
        for lane in [Lane::Cold, Lane::Hot, Lane::Trader] {
            let limiter = limiter.clone();
            tokio::spawn(async move { limiter.acquire(lane).await });
        }
        tokio::time::sleep(Duration::from_secs(15 * MIN - 1)).await;
        let s = limiter.snapshot();
        assert_eq!((s.granted_trader, s.granted_hot, s.granted_cold, s.waiting), (0, 0, 0, 3));
        tokio::time::sleep(Duration::from_secs(2)).await;
        let s = limiter.snapshot();
        assert_eq!((s.granted_trader, s.granted_hot, s.granted_cold), (1, 0, 0));
    }

    #[test]
    fn outcome_of_classifies_status_and_headers() {
        let headers = |pairs: &[(&'static str, &'static str)]| {
            let mut map = HeaderMap::new();
            for (k, v) in pairs {
                map.insert(*k, HeaderValue::from_static(v));
            }
            map
        };
        let json = headers(&[("content-type", "application/json")]);
        let html = headers(&[("content-type", "text/html; charset=UTF-8")]);
        let cf = headers(&[("cf-mitigated", "challenge")]);
        assert_eq!(outcome_of(200, &json), Outcome::Ok);
        assert_eq!(outcome_of(200, &html), Outcome::Ok);
        assert_eq!(outcome_of(404, &json), Outcome::Ok);
        assert_eq!(outcome_of(403, &html), Outcome::Challenge);
        assert_eq!(outcome_of(403, &cf), Outcome::Challenge);
        assert_eq!(outcome_of(503, &html), Outcome::Challenge);
        assert_eq!(outcome_of(429, &json), Outcome::RateLimited);
        assert_eq!(outcome_of(429, &html), Outcome::RateLimited);
        assert_eq!(outcome_of(500, &json), Outcome::TransportError);
        assert_eq!(outcome_of(502, &HeaderMap::new()), Outcome::TransportError);
        assert_eq!(outcome_of(502, &html), Outcome::TransportError, "a Cloudflare origin-error page is not a challenge");
        assert_eq!(outcome_of(520, &html), Outcome::TransportError);
        assert_eq!(outcome_of(400, &html), Outcome::Ok);
        assert_eq!(outcome_of(502, &cf), Outcome::Challenge);
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_a_waiting_acquire_unblocks_lower_lanes() {
        let limiter = Arc::new(Limiter::new(3));
        limiter.acquire(Lane::Cold).await;
        let trader = {
            let limiter = limiter.clone();
            tokio::spawn(async move { limiter.acquire(Lane::Trader).await })
        };
        tokio::task::yield_now().await;
        trader.abort();
        let _ = trader.await;
        let t = Instant::now();
        limiter.acquire(Lane::Cold).await;
        assert!(t.elapsed() <= Duration::from_millis(334));
        assert_eq!(limiter.snapshot().waiting, 0);
    }
}
