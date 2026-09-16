//! Daily database backup (spec §19 H6): `VACUUM INTO` a dated file, verify it on a read-only
//! connection, keep 7 dailies and 4 Sunday weeklies.

use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDate, Weekday};
use service::sea_orm::{ConnectionTrait, Database, DatabaseConnection};
use utils::{get_location, info, warning, Error, LoggerOptions};

use crate::collector::stmt;

const PREFIX: &str = "quantframe-";
const SUFFIX: &str = ".sqlite";
const TMP: &str = ".tmp";
pub const KEEP_DAILY: usize = 7;
pub const KEEP_WEEKLY: usize = 4;

pub fn file_name(date: NaiveDate) -> String {
    format!("{PREFIX}{}{SUFFIX}", date.format("%Y-%m-%d"))
}

/// The date in a backup file name; `None` for anything else in the directory.
pub fn parse_date(name: &str) -> Option<NaiveDate> {
    let middle = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    NaiveDate::parse_from_str(middle, "%Y-%m-%d").ok()
}

/// Dates to delete: keep the `KEEP_DAILY` newest, then the `KEEP_WEEKLY` newest Sundays among the rest.
pub fn to_delete(dates: &[NaiveDate]) -> Vec<NaiveDate> {
    let mut sorted = dates.to_vec();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    sorted.dedup();
    let rest = &sorted[sorted.len().min(KEEP_DAILY)..];
    let mut sundays_kept = 0;
    rest.iter()
        .copied()
        .filter(|date| {
            if date.weekday() == Weekday::Sun && sundays_kept < KEEP_WEEKLY {
                sundays_kept += 1;
                false
            } else {
                true
            }
        })
        .collect()
}

/// `VACUUM INTO` a fresh `.tmp` file beside the final name, so an unverified copy never wears it.
/// SQLite refuses to overwrite, so a stale temp left by a crashed run is deleted first.
pub async fn write(conn: &DatabaseConnection, dir: &Path, date: NaiveDate) -> Result<PathBuf, Error> {
    std::fs::create_dir_all(dir)
        .map_err(|e| Error::new("Housekeeping:Backup", format!("cannot create {}: {e}", dir.display()), get_location!()))?;
    let path = dir.join(format!("{}{TMP}", file_name(date)));
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| Error::new("Housekeeping:Backup", format!("cannot remove stale {}: {e}", path.display()), get_location!()))?;
    }
    let target = path.to_string_lossy();
    if target.contains('\'') {
        return Err(Error::new("Housekeeping:Backup", format!("backup path must not contain a quote: {target}"), get_location!()));
    }
    conn.execute_unprepared(&format!("VACUUM INTO '{target}'"))
        .await
        .map_err(|e| Error::new("Housekeeping:Backup", format!("VACUUM INTO {} failed: {e}", path.display()), get_location!()))?;
    Ok(path)
}

/// `PRAGMA integrity_check` on a second, read-only connection to the copy.
pub async fn verify(path: &Path) -> Result<(), Error> {
    let url = format!("sqlite://{}?mode=ro", path.display());
    let conn = Database::connect(url).await.map_err(|e| Error::new("Housekeeping:Verify", e.to_string(), get_location!()))?;
    let row = conn
        .query_one(stmt("PRAGMA integrity_check", vec![]))
        .await
        .map_err(|e| Error::new("Housekeeping:Verify", e.to_string(), get_location!()))?
        .ok_or_else(|| Error::new("Housekeeping:Verify", "integrity_check returned no row", get_location!()))?;
    let verdict: String = row.try_get("", "integrity_check").map_err(|e| Error::new("Housekeeping:Verify", e.to_string(), get_location!()))?;
    if verdict == "ok" {
        Ok(())
    } else {
        Err(Error::new("Housekeeping:Verify", format!("integrity_check: {verdict}"), get_location!()))
    }
}

/// Deletes dated backup files the retention rule no longer keeps. Other files are left alone.
pub fn prune(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| Error::new("Housekeeping:Prune", format!("cannot list {}: {e}", dir.display()), get_location!()))?;
    let dated: Vec<(NaiveDate, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| parse_date(&entry.file_name().to_string_lossy()).map(|date| (date, entry.path())))
        .collect();
    let dates: Vec<NaiveDate> = dated.iter().map(|(d, _)| *d).collect();
    let doomed = to_delete(&dates);
    let mut removed = Vec::new();
    for (date, path) in dated {
        if doomed.contains(&date) {
            std::fs::remove_file(&path)
                .map_err(|e| Error::new("Housekeeping:Prune", format!("cannot delete {}: {e}", path.display()), get_location!()))?;
            removed.push(path);
        }
    }
    Ok(removed)
}

/// Verifies a fresh copy and only then gives it the final name: a bad copy is removed, never renamed,
/// so a dated backup file is always one that passed `integrity_check`.
async fn promote(temp: &Path, final_path: &Path) -> Result<(), Error> {
    if let Err(e) = verify(temp).await {
        if let Err(rm) = std::fs::remove_file(temp) {
            warning("Housekeeping:Backup", format!("cannot remove bad copy {}: {rm}", temp.display()), &LoggerOptions::default());
        }
        return Err(e);
    }
    std::fs::rename(temp, final_path).map_err(|e| {
        Error::new("Housekeeping:Backup", format!("cannot rename {} to {}: {e}", temp.display(), final_path.display()), get_location!())
    })
}

/// The daily job: skip if today's file exists; else write a temp copy, verify and rename it, prune.
pub async fn run(conn: &DatabaseConnection, dir: &Path, today: NaiveDate) -> Result<Option<PathBuf>, Error> {
    let final_path = dir.join(file_name(today));
    if final_path.exists() {
        return Ok(None);
    }
    let temp = write(conn, dir, today).await?;
    promote(&temp, &final_path).await?;
    let removed = prune(dir)?;
    info(
        "Housekeeping:Backup",
        format!("Wrote {} and pruned {} old backup(s)", final_path.display(), removed.len()),
        &LoggerOptions::default(),
    );
    Ok(Some(final_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trader::store::tests::db;

    fn d(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn file_names_round_trip_and_ignore_strangers() {
        assert_eq!(file_name(d("2026-09-16")), "quantframe-2026-09-16.sqlite");
        assert_eq!(parse_date("quantframe-2026-09-16.sqlite"), Some(d("2026-09-16")));
        assert_eq!(parse_date("quantframe.sqlite"), None);
        assert_eq!(parse_date("quantframe-2026-09-16.sqlite.tmp"), None);
    }

    #[test]
    fn retention_keeps_seven_dailies_and_four_sundays() {
        // 40 consecutive days ending Wednesday 2026-09-16 (30 reach only 3 Sundays past the 7 dailies).
        let dates: Vec<NaiveDate> = (0..40).map(|i| d("2026-09-16") - chrono::Duration::days(i)).collect();
        let delete = to_delete(&dates);
        let kept: Vec<NaiveDate> = dates.iter().copied().filter(|x| !delete.contains(x)).collect();
        assert_eq!(kept.len(), 11);
        for i in 0..7 {
            assert!(kept.contains(&(d("2026-09-16") - chrono::Duration::days(i))), "the 7 newest survive");
        }
        for sunday in ["2026-09-06", "2026-08-30", "2026-08-23", "2026-08-16"] {
            assert!(kept.contains(&d(sunday)), "{sunday} is a kept Sunday");
        }
        assert!(!kept.contains(&d("2026-09-08")), "a weekday older than 7 days goes");
        // No Sunday in the tail: exactly 7 survive.
        let weekdays: Vec<NaiveDate> =
            (0..12).map(|i| d("2026-09-16") - chrono::Duration::days(i)).filter(|x| x.weekday() != chrono::Weekday::Sun).collect();
        assert_eq!(weekdays.len() - to_delete(&weekdays).len(), 7);
    }

    #[tokio::test]
    async fn vacuum_into_writes_a_consistent_copy_once_per_day_and_prunes() {
        let (dir, conn) = db().await;
        let backups = dir.path().join("backups");
        let today = d("2026-09-16");
        let written = run(&conn, &backups, today).await.unwrap().expect("first run writes");
        assert_eq!(written, backups.join("quantframe-2026-09-16.sqlite"));
        assert!(written.is_file());
        assert!(!backups.join("quantframe-2026-09-16.sqlite-wal").exists());
        assert!(!backups.join("quantframe-2026-09-16.sqlite-shm").exists());
        verify(&written).await.unwrap();
        assert_eq!(run(&conn, &backups, today).await.unwrap(), None, "today's file exists: nothing to do");

        // Old files are pruned by the rule, whatever their content.
        for i in 1..40 {
            std::fs::write(backups.join(file_name(today - chrono::Duration::days(i))), b"x").unwrap();
        }
        std::fs::write(backups.join("unrelated.txt"), b"x").unwrap();
        run(&conn, &backups, today + chrono::Duration::days(1)).await.unwrap().expect("next day writes again");
        let names: Vec<String> =
            std::fs::read_dir(&backups).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        let dated = names.iter().filter(|n| parse_date(n).is_some()).count();
        assert_eq!(dated, 11, "7 dailies + 4 Sundays");
        assert!(names.contains(&"unrelated.txt".to_string()), "only dated backup files are touched");
    }

    #[tokio::test]
    async fn a_bad_copy_is_removed_and_not_renamed() {
        let (dir, conn) = db().await;
        let backups = dir.path().join("backups");
        let today = d("2026-09-16");
        std::fs::create_dir_all(&backups).unwrap();
        let temp = backups.join("quantframe-2026-09-16.sqlite.tmp");
        let final_path = backups.join(file_name(today));

        // A copy that fails verification is deleted and never gets the dated name.
        std::fs::write(&temp, b"not a database").unwrap();
        assert!(promote(&temp, &final_path).await.is_err());
        assert!(!temp.exists(), "the bad copy is removed");
        assert!(!final_path.exists(), "and is never renamed into place");

        // A write that cannot happen (a directory sits on the temp path) leaves no final file either.
        std::fs::create_dir(&temp).unwrap();
        assert!(run(&conn, &backups, today).await.is_err());
        assert!(!final_path.exists(), "a failed run writes no dated backup");

        // A stale temp from a crashed run is replaced, and a good copy is promoted.
        std::fs::remove_dir(&temp).unwrap();
        std::fs::write(&temp, b"stale garbage").unwrap();
        assert_eq!(run(&conn, &backups, today).await.unwrap(), Some(final_path.clone()));
        assert!(final_path.is_file());
        assert!(!temp.exists(), "no .tmp is left behind");
    }

    #[tokio::test]
    async fn a_corrupt_file_fails_verification() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quantframe-2026-09-16.sqlite");
        std::fs::write(&path, b"not a database").unwrap();
        assert!(verify(&path).await.is_err());
    }
}
