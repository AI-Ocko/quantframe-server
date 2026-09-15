use std::collections::VecDeque;
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use super::store::ItemCounts;
use super::ts;
use crate::market::limiter::{Lane, LimiterSnapshot};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct LaneHealth {
    pub swept_last_hour: usize,
    pub errors_last_hour: usize,
    pub last_sweep_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CollectorHealth {
    pub running: bool,
    pub started_at: Option<String>,
    pub hot: LaneHealth,
    pub cold: LaneHealth,
    pub hot_items: usize,
    pub active_items: i64,
    pub inactive_items: i64,
    pub items_behind: i64,
    pub cold_pass_started_at: Option<String>,
    pub last_cold_pass_seconds: Option<i64>,
    pub last_error: Option<String>,
    pub last_item_refresh_at: Option<String>,
    pub last_maintenance_at: Option<String>,
    pub limiter: LimiterSnapshot,
}

struct Event {
    at: DateTime<Utc>,
    lane: Lane,
    ok: bool,
}

#[derive(Default)]
struct Inner {
    started_at: Option<DateTime<Utc>>,
    events: VecDeque<Event>,
    last_ok_hot: Option<DateTime<Utc>>,
    last_ok_cold: Option<DateTime<Utc>>,
    hot_items: usize,
    cold_pass_started_at: Option<DateTime<Utc>>,
    last_cold_pass_seconds: Option<i64>,
    last_error: Option<String>,
    last_item_refresh_at: Option<DateTime<Utc>>,
    last_maintenance_at: Option<DateTime<Utc>>,
}

/// In-memory collector health for the Market Data page. Lost on restart, which is fine.
#[derive(Default)]
pub struct HealthTracker {
    inner: Mutex<Inner>,
}

impl HealthTracker {
    pub fn mark_started(&self, at: DateTime<Utc>) {
        self.inner.lock().unwrap().started_at = Some(at);
    }

    pub fn record(&self, at: DateTime<Utc>, lane: Lane, ok: bool, error: Option<String>) {
        let mut inner = self.inner.lock().unwrap();
        inner.events.push_back(Event { at, lane, ok });
        while inner.events.front().is_some_and(|e| e.at < at - Duration::hours(1)) {
            inner.events.pop_front();
        }
        if ok {
            match lane {
                Lane::Hot => inner.last_ok_hot = Some(at),
                _ => inner.last_ok_cold = Some(at),
            }
        }
        if let Some(error) = error {
            inner.last_error = Some(error);
        }
    }

    pub fn set_hot_items(&self, n: usize) {
        self.inner.lock().unwrap().hot_items = n;
    }

    pub fn cold_pass_start_or_init(&self, at: DateTime<Utc>) -> DateTime<Utc> {
        *self.inner.lock().unwrap().cold_pass_started_at.get_or_insert(at)
    }

    pub fn finish_cold_pass(&self, at: DateTime<Utc>) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(started) = inner.cold_pass_started_at {
            inner.last_cold_pass_seconds = Some((at - started).num_seconds());
        }
        inner.cold_pass_started_at = Some(at);
    }

    pub fn last_cold_pass_seconds(&self) -> Option<i64> {
        self.inner.lock().unwrap().last_cold_pass_seconds
    }

    pub fn item_refresh_done(&self, at: DateTime<Utc>) {
        self.inner.lock().unwrap().last_item_refresh_at = Some(at);
    }

    pub fn maintenance_done(&self, at: DateTime<Utc>) {
        self.inner.lock().unwrap().last_maintenance_at = Some(at);
    }

    pub fn lane_health(&self, lane: Lane, now: DateTime<Utc>) -> LaneHealth {
        let inner = self.inner.lock().unwrap();
        let recent = inner.events.iter().filter(|e| e.lane == lane && e.at >= now - Duration::hours(1));
        let (ok, failed): (Vec<&Event>, Vec<&Event>) = recent.partition(|e| e.ok);
        let last = match lane {
            Lane::Hot => inner.last_ok_hot,
            _ => inner.last_ok_cold,
        };
        LaneHealth { swept_last_hour: ok.len(), errors_last_hour: failed.len(), last_sweep_at: last.map(ts) }
    }

    pub fn snapshot(&self, now: DateTime<Utc>, counts: ItemCounts, limiter: LimiterSnapshot) -> CollectorHealth {
        let hot = self.lane_health(Lane::Hot, now);
        let cold = self.lane_health(Lane::Cold, now);
        let inner = self.inner.lock().unwrap();
        CollectorHealth {
            running: inner.started_at.is_some(),
            started_at: inner.started_at.map(ts),
            hot,
            cold,
            hot_items: inner.hot_items,
            active_items: counts.active,
            inactive_items: counts.inactive,
            items_behind: counts.behind,
            cold_pass_started_at: inner.cold_pass_started_at.map(ts),
            last_cold_pass_seconds: inner.last_cold_pass_seconds,
            last_error: inner.last_error.clone(),
            last_item_refresh_at: inner.last_item_refresh_at.map(ts),
            last_maintenance_at: inner.last_maintenance_at.map(ts),
            limiter,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;

    fn at(minutes: i64) -> DateTime<Utc> {
        parse_ts("2026-09-15T00:00:00Z").unwrap() + Duration::minutes(minutes)
    }

    #[test]
    fn lane_health_counts_the_last_hour_only() {
        let health = HealthTracker::default();
        health.record(at(0), Lane::Cold, true, None);
        health.record(at(30), Lane::Cold, false, Some("slug: HTTP 502".into()));
        health.record(at(61), Lane::Cold, true, None);
        health.record(at(61), Lane::Hot, true, None);
        let cold = health.lane_health(Lane::Cold, at(65));
        assert_eq!(cold, LaneHealth { swept_last_hour: 1, errors_last_hour: 1, last_sweep_at: Some("2026-09-15T01:01:00Z".into()) });
        assert_eq!(health.lane_health(Lane::Hot, at(65)).swept_last_hour, 1);
    }

    #[test]
    fn cold_pass_duration_is_measured_between_passes() {
        let health = HealthTracker::default();
        assert_eq!(health.cold_pass_start_or_init(at(0)), at(0));
        assert_eq!(health.cold_pass_start_or_init(at(5)), at(0));
        health.finish_cold_pass(at(25));
        assert_eq!(health.last_cold_pass_seconds(), Some(1500));
        assert_eq!(health.cold_pass_start_or_init(at(26)), at(25));
    }

    #[test]
    fn snapshot_combines_counts_errors_and_limiter() {
        let health = HealthTracker::default();
        health.mark_started(at(0));
        health.set_hot_items(4);
        health.record(at(1), Lane::Hot, false, Some("boom".into()));
        let limiter = crate::market::limiter::Limiter::new(3).snapshot();
        let snap = health.snapshot(at(2), ItemCounts { active: 10, inactive: 1, behind: 3 }, limiter);
        assert!(snap.running);
        assert_eq!(snap.hot_items, 4);
        assert_eq!(snap.active_items, 10);
        assert_eq!(snap.items_behind, 3);
        assert_eq!(snap.last_error.as_deref(), Some("boom"));
        assert_eq!(snap.hot.errors_last_hour, 1);
    }
}
