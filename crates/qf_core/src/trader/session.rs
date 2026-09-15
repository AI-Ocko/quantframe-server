//! warframe.market session health for the trader lifecycle (amendments C7, C10, C12).

use std::sync::{Mutex, OnceLock};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use utils::{error, LoggerOptions};
use wf_market::errors::ApiError;

use crate::collector::ts;
use crate::utils::modules::states;

pub const ME_CHECK_EVERY: StdDuration = StdDuration::from_secs(15 * 60);
pub const TOKEN_FRESH_MINUTES: i64 = 20;
pub const EXPIRY_WARNING_DAYS: i64 = 7;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SessionSnapshot {
    pub signed_in: bool,
    pub token_valid: bool,
    pub unauthorized: bool,
    pub ws_connected: bool,
    pub ws_down_for_s: Option<i64>,
    pub last_me_ok_at: Option<String>,
    pub token_expires_at: Option<String>,
}

#[derive(Default)]
struct Inner {
    signed_in: bool,
    unauthorized: bool,
    last_me_ok: Option<DateTime<Utc>>,
    ws_connected: bool,
    ws_down_since: Option<DateTime<Utc>>,
    token_expires_at: Option<DateTime<Utc>>,
    last_expiry_alert: Option<DateTime<Utc>>,
}

#[derive(Default)]
pub struct Session {
    inner: Mutex<Inner>,
}

static SESSION: OnceLock<Session> = OnceLock::new();

pub fn get() -> &'static Session {
    SESSION.get_or_init(Session::default)
}

impl Session {
    pub fn mark_signed_in(&self, at: DateTime<Utc>, token_expires_at: Option<DateTime<Utc>>) {
        let mut inner = self.inner.lock().unwrap();
        inner.signed_in = true;
        inner.unauthorized = false;
        inner.last_me_ok = Some(at);
        inner.token_expires_at = token_expires_at;
        inner.last_expiry_alert = None;
    }

    pub fn mark_signed_out(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.signed_in = false;
        inner.unauthorized = false;
        inner.last_me_ok = None;
        inner.token_expires_at = None;
        inner.last_expiry_alert = None;
    }

    pub fn mark_me_ok(&self, at: DateTime<Utc>) {
        let mut inner = self.inner.lock().unwrap();
        if inner.signed_in {
            inner.last_me_ok = Some(at);
            inner.unauthorized = false;
        }
    }

    pub fn mark_unauthorized(&self) {
        self.inner.lock().unwrap().unauthorized = true;
    }

    pub fn set_ws_connected(&self, connected: bool, at: DateTime<Utc>) {
        let mut inner = self.inner.lock().unwrap();
        if connected {
            inner.ws_connected = true;
            inner.ws_down_since = None;
        } else {
            inner.ws_connected = false;
            if inner.ws_down_since.is_none() {
                inner.ws_down_since = Some(at);
            }
        }
    }

    pub fn snapshot(&self, now: DateTime<Utc>) -> SessionSnapshot {
        let inner = self.inner.lock().unwrap();
        SessionSnapshot {
            signed_in: inner.signed_in,
            token_valid: inner.signed_in
                && !inner.unauthorized
                && inner.last_me_ok.is_some_and(|t| now - t <= Duration::minutes(TOKEN_FRESH_MINUTES)),
            unauthorized: inner.unauthorized,
            ws_connected: inner.ws_connected,
            ws_down_for_s: if inner.ws_connected { None } else { inner.ws_down_since.map(|t| (now - t).num_seconds()) },
            last_me_ok_at: inner.last_me_ok.map(ts),
            token_expires_at: inner.token_expires_at.map(ts),
        }
    }

    /// The token expiry when an alert is due (expires within 7 days, none sent in 24 h). Records the alert.
    pub fn expiry_alert_due(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let mut inner = self.inner.lock().unwrap();
        let expires = inner.token_expires_at?;
        if !inner.signed_in || expires - now > Duration::days(EXPIRY_WARNING_DAYS) {
            return None;
        }
        if inner.last_expiry_alert.is_some_and(|t| now - t < Duration::hours(24)) {
            return None;
        }
        inner.last_expiry_alert = Some(now);
        Some(expires)
    }
}

/// Checks `/me` every 15 minutes while signed in (spec §5.1).
pub async fn me_check_loop() {
    loop {
        tokio::time::sleep(ME_CHECK_EVERY).await;
        check_me_once().await;
    }
}

pub async fn check_me_once() {
    let session = get();
    if !session.snapshot(Utc::now()).signed_in {
        return;
    }
    let Some(app) = states::try_app_state() else { return };
    match app.wfm_client.user().me().await {
        Ok(_) => session.mark_me_ok(Utc::now()),
        Err(ApiError::Unauthorized(_)) => session.mark_unauthorized(),
        Err(e) => error("Trader:Session", format!("/me check failed: {}", e), &LoggerOptions::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;

    fn at(minutes: i64) -> DateTime<Utc> {
        parse_ts("2026-09-16T00:00:00Z").unwrap() + Duration::minutes(minutes)
    }

    #[test]
    fn token_is_valid_only_while_signed_in_fresh_and_authorised() {
        let session = Session::default();
        assert!(!session.snapshot(at(0)).token_valid);
        session.mark_signed_in(at(0), None);
        assert!(session.snapshot(at(20)).token_valid);
        assert!(!session.snapshot(at(21)).token_valid, "stale /me");
        session.mark_me_ok(at(21));
        assert!(session.snapshot(at(30)).token_valid);
        session.mark_unauthorized();
        let snap = session.snapshot(at(30));
        assert!(snap.unauthorized && !snap.token_valid);
        session.mark_me_ok(at(31));
        assert!(session.snapshot(at(31)).token_valid, "a good /me clears the 401");
        session.mark_signed_out();
        assert!(!session.snapshot(at(31)).signed_in);
    }

    #[test]
    fn websocket_down_time_counts_from_the_first_disconnect() {
        let session = Session::default();
        assert_eq!(session.snapshot(at(0)).ws_down_for_s, None, "unknown before any callback");
        session.set_ws_connected(true, at(0));
        session.set_ws_connected(false, at(1));
        session.set_ws_connected(false, at(2));
        assert_eq!(session.snapshot(at(3)).ws_down_for_s, Some(120));
        session.set_ws_connected(true, at(4));
        let snap = session.snapshot(at(5));
        assert!(snap.ws_connected);
        assert_eq!(snap.ws_down_for_s, None);
    }

    #[test]
    fn expiry_alert_fires_within_seven_days_at_most_daily() {
        let session = Session::default();
        session.mark_signed_in(at(0), Some(at(0) + Duration::days(10)));
        assert_eq!(session.expiry_alert_due(at(0)), None, "10 days left");
        let four_days_later = at(0) + Duration::days(4);
        assert!(session.expiry_alert_due(four_days_later).is_some());
        assert_eq!(session.expiry_alert_due(four_days_later + Duration::hours(23)), None);
        assert!(session.expiry_alert_due(four_days_later + Duration::hours(24)).is_some());
    }
}
