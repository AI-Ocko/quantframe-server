//! Always-on maintenance that must not depend on the collector (spec §19 H5): the alert sweep every
//! tick, `helper_events` retention hourly, and the daily backup (H6).

use std::path::{Path, PathBuf};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use service::sea_orm::DatabaseConnection;
use utils::{error, info, Error, LoggerOptions};

use crate::helper_link::trades::{self, events, TradeEnv};

pub mod backup;

pub const TICK: StdDuration = StdDuration::from_secs(60);
const RESTART_DELAY: StdDuration = StdDuration::from_secs(5);

fn hourly() -> Duration {
    Duration::hours(1)
}

#[derive(Debug, Default)]
pub struct Gates {
    pub last_hourly: Option<DateTime<Utc>>,
    /// A failed backup is not retried until the next UTC day (H6).
    pub backup_failed_on: Option<NaiveDate>,
}

#[derive(Debug, Default, PartialEq)]
pub struct TickReport {
    pub alerts: usize,
    pub deleted_events: Option<u64>,
    pub backup: Option<PathBuf>,
}

/// One housekeeping pass. The three jobs are independent: one failing must not starve the others,
/// so the first error is returned only after all of them were attempted. `backup_dir = None` skips
/// the backup job (tests).
pub async fn tick(
    conn: &DatabaseConnection,
    env: &dyn TradeEnv,
    now: DateTime<Utc>,
    gates: &mut Gates,
    backup_dir: Option<&Path>,
) -> Result<TickReport, Error> {
    let mut report = TickReport::default();
    let mut first_error: Option<Error> = None;
    match trades::sweep_alerts(conn, env, now).await {
        Ok(alerts) => report.alerts = alerts,
        Err(e) => {
            first_error.get_or_insert(e);
        }
    }
    if gates.last_hourly.is_none_or(|t| now - t >= hourly()) {
        match events::apply_retention(conn, now).await {
            Ok(deleted) => {
                report.deleted_events = Some(deleted);
                gates.last_hourly = Some(now);
            }
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    if let Some(dir) = backup_dir {
        let today = now.date_naive();
        if gates.backup_failed_on != Some(today) {
            match backup::run(conn, dir, today).await {
                Ok(written) => report.backup = written,
                Err(e) => {
                    gates.backup_failed_on = Some(today);
                    alert_backup_failed(&e.message, now);
                }
            }
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(report),
    }
}

/// Critical log, red toast and `on_alert` with `<KIND> = backup_failed` (H6). No retry until the next UTC day.
pub fn alert_backup_failed(reason: &str, now: DateTime<Utc>) {
    use serde_json::json;
    use std::collections::HashMap;
    error("Housekeeping:Backup", format!("Backup failed: {reason}"), &LoggerOptions::default());
    crate::notify_gui!("on_backup", "red", "failed", json!({"reason": reason}), json!({"autoClose": false}));
    if let Some(app) = crate::utils::modules::states::try_app_state() {
        let variables = HashMap::from([
            ("<KIND>".to_string(), "backup_failed".to_string()),
            ("<REASON>".to_string(), reason.to_string()),
            ("<TIME>".to_string(), crate::collector::ts(now)),
            ("<PLAYER_NAME>".to_string(), String::new()),
            ("<EVENT_ID>".to_string(), String::new()),
        ]);
        app.settings.notifications.on_alert.send(&variables, Some(json!({"event": "backup_failed", "reason": reason, "at": crate::collector::ts(now)})));
    }
}

async fn run_loop(conn: DatabaseConnection, backup_dir: PathBuf) {
    let env = crate::helper_link::trades::live::LiveEnv;
    let mut gates = Gates::default();
    loop {
        match tick(&conn, &env, Utc::now(), &mut gates, Some(&backup_dir)).await {
            Ok(report) => {
                if report.alerts > 0 || report.deleted_events.is_some() || report.backup.is_some() {
                    info(
                        "Housekeeping",
                        format!(
                            "alerts {}, deleted events {}, backup {}",
                            report.alerts,
                            report.deleted_events.map_or("-".to_string(), |n| n.to_string()),
                            report.backup.as_ref().map_or("-".to_string(), |p| p.display().to_string())
                        ),
                        &LoggerOptions::default(),
                    );
                }
            }
            Err(e) => error("Housekeeping", format!("{} ({})", e.message, e.component), &LoggerOptions::default()),
        }
        tokio::time::sleep(TICK).await;
    }
}

/// Starts the supervised loop. Unconditional: it does not depend on `QF_COLLECTOR` (H5).
pub fn start(conn: DatabaseConnection, backup_dir: PathBuf) {
    let dir = backup_dir.clone();
    crate::collector::runner::supervise("Housekeeping", RESTART_DELAY, move || run_loop(conn.clone(), dir.clone()));
    info("Housekeeping", format!("Started: tick {} s, backups in {}", TICK.as_secs(), backup_dir.display()), &LoggerOptions::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_link::trades::events::tests::{at, event};
    use crate::helper_link::trades::events::{get, insert, list, NEEDS_REVIEW};
    use crate::trader::store::tests::db;

    #[tokio::test]
    async fn a_tick_sweeps_alerts_and_runs_retention_once_an_hour() {
        let (_dir, conn) = db().await;
        let env = crate::helper_link::trades::tests::fake(true);
        let mut old = event("old", "2026-06-01T00:00:00Z", NEEDS_REVIEW);
        old.reason = Some("unresolved: x".into());
        insert(&conn, &old).await.unwrap();
        let mut failed = event("failed", "2026-09-16T12:00:00Z", NEEDS_REVIEW);
        failed.reason = Some("apply_failed: HandleItem".into());
        insert(&conn, &failed).await.unwrap();

        let mut gates = Gates::default();
        let first = tick(&conn, &env, at("2026-09-16T12:01:00Z"), &mut gates, None).await.unwrap();
        assert_eq!(first, TickReport { alerts: 1, deleted_events: Some(1), backup: None }, "first tick runs the hourly job");
        assert_eq!(list(&conn, None, 1, 10).await.unwrap().total, 1, "the 107-day-old event is gone");

        let second = tick(&conn, &env, at("2026-09-16T12:02:00Z"), &mut gates, None).await.unwrap();
        assert_eq!(second, TickReport { alerts: 0, deleted_events: None, backup: None }, "one minute later: no hourly job, nothing new to alert");

        let third = tick(&conn, &env, at("2026-09-16T13:01:00Z"), &mut gates, None).await.unwrap();
        assert_eq!(third.deleted_events, Some(0), "an hour later the hourly job runs again");
    }

    #[tokio::test]
    async fn a_broken_event_row_does_not_stop_retention() {
        let (_dir, conn) = db().await;
        let env = crate::helper_link::trades::tests::fake(true);
        let mut old = event("old", "2026-06-01T00:00:00Z", NEEDS_REVIEW);
        old.reason = Some("unresolved: x".into());
        insert(&conn, &old).await.unwrap();
        for id in ["failed", "intact"] {
            let mut failed = event(id, "2026-09-16T12:00:00Z", NEEDS_REVIEW);
            failed.reason = Some("apply_failed: HandleItem".into());
            insert(&conn, &failed).await.unwrap();
        }
        crate::collector::store::exec(
            &conn,
            "Test",
            "UPDATE helper_events SET payload = 'not json' WHERE event_id = ?",
            vec!["failed".into()],
        )
        .await
        .unwrap();

        let mut gates = Gates::default();
        let report = tick(&conn, &env, at("2026-09-16T12:01:00Z"), &mut gates, None).await.unwrap();

        assert_eq!(report.alerts, 1, "the intact row still alerts; only the corrupt one is skipped");
        assert!(get(&conn, "old").await.unwrap().is_none(), "retention ran alongside the tolerant sweep");
    }

    #[tokio::test]
    async fn a_failed_backup_alerts_once_per_day() {
        let (dir, conn) = db().await;
        let env = crate::helper_link::trades::tests::fake(true);
        // A file where the backup directory should be: create_dir_all fails, so the backup cannot run.
        let not_a_dir = dir.path().join("notadir");
        std::fs::write(&not_a_dir, b"x").unwrap();
        let day = at("2026-09-16T12:00:00Z").date_naive();

        let mut gates = Gates::default();
        for minute in ["00", "01", "02"] {
            let now = at(&format!("2026-09-16T12:{minute}:00Z"));
            let report = tick(&conn, &env, now, &mut gates, Some(&not_a_dir)).await.unwrap();
            assert_eq!(report.backup, None, "a failed backup writes nothing");
            assert_eq!(gates.backup_failed_on, Some(day), "the gate stays set, so ticks 2 and 3 never call run() again");
        }

        let next_day = at("2026-09-17T00:01:00Z");
        let report = tick(&conn, &env, next_day, &mut gates, Some(&not_a_dir)).await.unwrap();
        assert_eq!(report.backup, None);
        assert_eq!(gates.backup_failed_on, Some(next_day.date_naive()), "the next UTC day tries again");
    }
}
