//! Market data collector (spec §5.4, §5.5 and amendments §15).

pub mod diff;
pub mod orders;

use chrono::{DateTime, SecondsFormat, Utc};
use service::sea_orm::{DbBackend, Statement, Value};
use utils::{get_location, Error};

/// Formats a collector timestamp. Every collector column uses this form, so text comparison orders correctly.
pub fn ts(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub fn parse_ts(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text).ok().map(|d| d.with_timezone(&Utc))
}

pub(crate) fn db_err(component: &str, e: impl std::fmt::Display) -> Error {
    Error::new(component, e.to_string(), get_location!())
}

pub(crate) fn stmt(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_round_trip_in_whole_seconds_with_z() {
        let at = parse_ts("2026-09-15T01:02:03.987+00:00").unwrap();
        assert_eq!(ts(at), "2026-09-15T01:02:03Z");
        assert_eq!(parse_ts(&ts(at)).unwrap().timestamp(), at.timestamp());
        assert!(parse_ts("not a time").is_none());
    }
}
