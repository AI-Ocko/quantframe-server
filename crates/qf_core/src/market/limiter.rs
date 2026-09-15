//! One shared warframe.market request budget with strict priority lanes (spec §5.3).

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Notify;
use tokio::time::Instant;

pub const RATE_PER_SECOND: u32 = 3;
const BACKOFF_START: Duration = Duration::from_secs(5);
const BACKOFF_MAX: Duration = Duration::from_secs(60);
const BACKOFF_RESET_AFTER: Duration = Duration::from_secs(60);

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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LimiterSnapshot {
    pub granted_trader: u64,
    pub granted_hot: u64,
    pub granted_cold: u64,
    pub waiting: usize,
    pub paused_ms: u64,
    pub rate_limited_total: u64,
}

struct State {
    next_slot: Option<Instant>,
    pause_until: Option<Instant>,
    backoff: Duration,
    waiting: [usize; 3],
    granted: [u64; 3],
    rate_limited_total: u64,
}

pub struct Limiter {
    interval: Duration,
    state: Mutex<State>,
    notify: Notify,
}

static GLOBAL: OnceLock<Limiter> = OnceLock::new();

/// The process-wide limiter every warframe.market REST call goes through.
pub fn global() -> &'static Limiter {
    GLOBAL.get_or_init(|| Limiter::new(RATE_PER_SECOND))
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
    pub fn new(rate_per_second: u32) -> Self {
        Self {
            interval: Duration::from_secs(1) / rate_per_second.max(1),
            state: Mutex::new(State {
                next_slot: None,
                pause_until: None,
                backoff: BACKOFF_START,
                waiting: [0; 3],
                granted: [0; 3],
                rate_limited_total: 0,
            }),
            notify: Notify::new(),
        }
    }

    /// Waits for a request slot. A lane is served only while no higher lane is waiting.
    pub async fn acquire(&self, lane: Lane) {
        self.state.lock().unwrap().waiting[lane.index()] += 1;
        let mut waiting = Waiting { limiter: self, lane, granted: false };
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let wake_at = {
                let mut s = self.state.lock().unwrap();
                if s.waiting[..lane.index()].iter().any(|&n| n > 0) {
                    None
                } else {
                    let now = Instant::now();
                    let mut ready = s.next_slot.unwrap_or(now);
                    if let Some(pause) = s.pause_until {
                        ready = ready.max(pause);
                    }
                    if ready <= now {
                        s.next_slot = Some(now + self.interval);
                        s.waiting[lane.index()] -= 1;
                        s.granted[lane.index()] += 1;
                        waiting.granted = true;
                        drop(s);
                        self.notify.notify_waiters();
                        return;
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

    /// Called after a 429. Pauses every lane for 5 s, doubling up to 60 s while 429s keep
    /// arriving within 60 s of the previous pause ending.
    pub fn report_429(&self) {
        let now = Instant::now();
        {
            let mut s = self.state.lock().unwrap();
            s.backoff = match s.pause_until {
                Some(end) if now < end + BACKOFF_RESET_AFTER && s.rate_limited_total > 0 => {
                    (s.backoff * 2).min(BACKOFF_MAX)
                }
                _ => BACKOFF_START,
            };
            s.pause_until = Some(now + s.backoff);
            s.rate_limited_total += 1;
        }
        self.notify.notify_waiters();
    }

    pub fn snapshot(&self) -> LimiterSnapshot {
        let s = self.state.lock().unwrap();
        let now = Instant::now();
        LimiterSnapshot {
            granted_trader: s.granted[Lane::Trader.index()],
            granted_hot: s.granted[Lane::Hot.index()],
            granted_cold: s.granted[Lane::Cold.index()],
            waiting: s.waiting.iter().sum(),
            paused_ms: s
                .pause_until
                .map(|end| end.saturating_duration_since(now).as_millis() as u64)
                .unwrap_or(0),
            rate_limited_total: s.rate_limited_total,
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

    #[tokio::test(start_paused = true)]
    async fn a_429_pauses_every_lane_and_backoff_doubles_then_resets() {
        let limiter = Limiter::new(3);
        limiter.report_429();
        let t = Instant::now();
        limiter.acquire(Lane::Trader).await;
        assert_eq!(t.elapsed().as_secs(), 5);

        limiter.report_429();
        let t = Instant::now();
        limiter.acquire(Lane::Cold).await;
        assert_eq!(t.elapsed().as_secs(), 10);

        tokio::time::advance(Duration::from_secs(61)).await;
        limiter.report_429();
        let t = Instant::now();
        limiter.acquire(Lane::Hot).await;
        assert_eq!(t.elapsed().as_secs(), 5);
        assert_eq!(limiter.snapshot().rate_limited_total, 3);
    }

    #[tokio::test(start_paused = true)]
    async fn backoff_is_capped_at_sixty_seconds() {
        let limiter = Limiter::new(3);
        for _ in 0..8 {
            limiter.report_429();
        }
        assert_eq!(limiter.snapshot().paused_ms, 60_000);
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
