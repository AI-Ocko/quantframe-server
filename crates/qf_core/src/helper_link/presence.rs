//! Latest qf-helper heartbeat, kept in memory (spec §5.7, amendment D4). A restart clears it.

use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::collector::ts;

/// Ready needs a heartbeat at most this old.
pub const READY_WITHIN_S: i64 = 30;
/// Trading stops when the last heartbeat is older than this.
pub const SILENT_AFTER_S: i64 = 60;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Heartbeat {
    pub warframe_running: bool,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HelperSnapshot {
    /// A heartbeat arrived within `READY_WITHIN_S`.
    pub connected: bool,
    /// As reported by the latest heartbeat; false when there has been none.
    pub warframe_running: bool,
    pub seconds_since_heartbeat: Option<i64>,
    pub last_heartbeat_at: Option<String>,
    pub device_name: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
struct Last {
    at: DateTime<Utc>,
    device_name: String,
    heartbeat: Heartbeat,
}

#[derive(Default)]
pub struct Presence {
    last: Mutex<Option<Last>>,
}

static PRESENCE: OnceLock<Presence> = OnceLock::new();

pub fn get() -> &'static Presence {
    PRESENCE.get_or_init(Presence::default)
}

impl Presence {
    pub fn record(&self, device_name: &str, heartbeat: Heartbeat, at: DateTime<Utc>) {
        *self.last.lock().unwrap() = Some(Last { at, device_name: device_name.to_string(), heartbeat });
    }

    pub fn snapshot(&self, now: DateTime<Utc>) -> HelperSnapshot {
        let last = self.last.lock().unwrap();
        let Some(last) = last.as_ref() else { return HelperSnapshot::default() };
        let since = (now - last.at).num_seconds().max(0);
        HelperSnapshot {
            connected: since <= READY_WITHIN_S,
            warframe_running: last.heartbeat.warframe_running,
            seconds_since_heartbeat: Some(since),
            last_heartbeat_at: Some(ts(last.at)),
            device_name: Some(last.device_name.clone()),
            version: Some(last.heartbeat.version.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;
    use chrono::Duration;

    fn at(seconds: i64) -> DateTime<Utc> {
        parse_ts("2026-09-17T10:00:00Z").unwrap() + Duration::seconds(seconds)
    }

    fn beat(running: bool) -> Heartbeat {
        Heartbeat { warframe_running: running, version: "0.1.0".into() }
    }

    #[test]
    fn no_heartbeat_means_not_connected() {
        assert_eq!(Presence::default().snapshot(at(0)), HelperSnapshot::default());
    }

    #[test]
    fn connected_for_thirty_seconds_after_a_heartbeat() {
        let presence = Presence::default();
        presence.record("gaming-pc", beat(true), at(0));
        let fresh = presence.snapshot(at(30));
        assert!(fresh.connected && fresh.warframe_running);
        assert_eq!(fresh.seconds_since_heartbeat, Some(30));
        assert_eq!(fresh.device_name.as_deref(), Some("gaming-pc"));
        assert_eq!(fresh.version.as_deref(), Some("0.1.0"));
        assert_eq!(fresh.last_heartbeat_at.as_deref(), Some("2026-09-17T10:00:00Z"));

        let stale = presence.snapshot(at(31));
        assert!(!stale.connected && stale.warframe_running, "stale keeps the last reported game state");
        assert_eq!(stale.seconds_since_heartbeat, Some(31));
    }

    #[test]
    fn the_latest_heartbeat_wins() {
        let presence = Presence::default();
        presence.record("gaming-pc", beat(true), at(0));
        presence.record("laptop", beat(false), at(10));
        let snap = presence.snapshot(at(12));
        assert!(snap.connected && !snap.warframe_running);
        assert_eq!(snap.device_name.as_deref(), Some("laptop"));
    }

    #[test]
    fn heartbeat_json_needs_warframe_running() {
        let ok: Heartbeat = serde_json::from_str(r#"{"warframe_running": true}"#).unwrap();
        assert_eq!(ok, Heartbeat { warframe_running: true, version: String::new() });
        assert!(serde_json::from_str::<Heartbeat>(r#"{"version": "0.1.0"}"#).is_err());
    }
}
