//! warframe.market closed-trade dailies as a price basis (spec §25 P1–P3).

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use service::sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};
use utils::{get_location, warning, Error, LoggerOptions};

use super::backfill::{fetch_with_retries, parse_statistics, ClosedDay, StatisticsSource};
use super::fetch::FetchError;
use super::store::exec;
use super::{db_err, stmt, ts};
use crate::market::limiter::{Lane, Limiter};

pub const WINDOW_DAYS: i64 = 7;
pub const WARM_MIN_DAYS: usize = 5;
pub const FRESH_DAYS: i64 = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct ClosedRow {
    pub item_id: String,
    pub sub_type: String,
    pub day: NaiveDate,
    pub volume: i64,
    pub median: Option<f64>,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
    pub wa_price: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClosedStats {
    pub item_id: String,
    pub sub_type: String,
    pub volume: f64,
    pub moving_avg: Option<f64>,
    pub median: Option<f64>,
    pub avg_price: Option<f64>,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
    pub week_price_shift: Option<f64>,
    pub days: usize,
    pub trades: i64,
    pub warm: bool,
}

fn median_f64(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let mid = sorted.len() / 2;
    Some(if sorted.len() % 2 == 0 { (sorted[mid - 1] + sorted[mid]) / 2.0 } else { sorted[mid] })
}

/// Closed stats over the seven UTC days before `today` (spec §25 P3). Rows outside that window are ignored.
pub fn aggregate(rows: Vec<ClosedRow>, today: NaiveDate, warm_min_trades: usize) -> Vec<ClosedStats> {
    let first = today - Duration::days(WINDOW_DAYS);
    let mut by_key: BTreeMap<(String, String), Vec<ClosedRow>> = BTreeMap::new();
    for r in rows.into_iter().filter(|r| r.day >= first && r.day < today) {
        by_key.entry((r.item_id.clone(), r.sub_type.clone())).or_default().push(r);
    }
    by_key
        .into_iter()
        .map(|((item_id, sub_type), mut days)| {
            days.sort_by_key(|r| r.day);
            let medians: Vec<f64> = days.iter().filter_map(|r| r.median).collect();
            let trades: i64 = days.iter().map(|r| r.volume).sum();
            let moving_avg = (!medians.is_empty()).then(|| medians.iter().sum::<f64>() / medians.len() as f64);
            let recent: Vec<&ClosedRow> = days.iter().rev().filter(|r| r.volume > 0 && r.wa_price.is_some()).take(2).collect();
            let recent_volume: i64 = recent.iter().map(|r| r.volume).sum();
            let avg_price = if recent_volume > 0 {
                Some(recent.iter().map(|r| r.wa_price.unwrap_or(0.0) * r.volume as f64).sum::<f64>() / recent_volume as f64)
            } else {
                moving_avg
            };
            let with_median: Vec<&ClosedRow> = days.iter().filter(|r| r.median.is_some()).collect();
            let week_price_shift = match (with_median.first(), with_median.last()) {
                (Some(a), Some(b)) if a.day != b.day => Some(b.median.unwrap_or(0.0) - a.median.unwrap_or(0.0)),
                _ => None,
            };
            ClosedStats {
                item_id,
                sub_type,
                volume: trades as f64 / WINDOW_DAYS as f64,
                moving_avg,
                median: median_f64(&medians),
                avg_price,
                min_price: days.iter().filter_map(|r| r.min_price).min(),
                max_price: days.iter().filter_map(|r| r.max_price).max(),
                week_price_shift,
                days: days.len(),
                trades,
                warm: days.len() >= WARM_MIN_DAYS && trades >= warm_min_trades as i64,
            }
        })
        .collect()
}

/// Upserts one item's days; this table has a single kind of writer (spec §25 P1).
pub async fn upsert_days(conn: &DatabaseConnection, item_id: &str, days: &[ClosedDay]) -> Result<u64, Error> {
    const C: &str = "ClosedStats:Upsert";
    let txn = conn.begin().await.map_err(|e| db_err(C, e))?;
    let mut written = 0;
    for d in days {
        written += txn
            .execute(stmt(
                "INSERT INTO closed_stats_daily (item_id, sub_type, day, volume, median, min_price, max_price, avg_price, wa_price)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (item_id, sub_type, day) DO UPDATE SET volume = excluded.volume, median = excluded.median,
                   min_price = excluded.min_price, max_price = excluded.max_price, avg_price = excluded.avg_price, wa_price = excluded.wa_price",
                vec![item_id.into(), d.sub_type.clone().into(), d.day.clone().into(), d.volume.into(), d.median.into(), d.min_price.into(), d.max_price.into(), d.avg_price.into(), d.wa_price.into()],
            ))
            .await
            .map_err(|e| db_err(C, e))?
            .rows_affected();
    }
    txn.commit().await.map_err(|e| db_err(C, e))?;
    Ok(written)
}

pub async fn set_fetch_state(conn: &DatabaseConnection, item_id: &str, at: DateTime<Utc>, outcome: &str) -> Result<(), Error> {
    conn.execute(stmt(
        "INSERT INTO closed_fetch_state (item_id, fetched_at, outcome) VALUES (?, ?, ?)
         ON CONFLICT (item_id) DO UPDATE SET fetched_at = excluded.fetched_at, outcome = excluded.outcome",
        vec![item_id.into(), ts(at).into(), outcome.into()],
    ))
    .await
    .map_err(|e| db_err("ClosedStats:State", e))?;
    Ok(())
}

/// Closed stats for every item whose last fetch was `ok` within `FRESH_DAYS` (spec §25 P3).
pub async fn load_fresh(conn: &DatabaseConnection, now: DateTime<Utc>, warm_min_trades: usize) -> Result<Vec<ClosedStats>, Error> {
    const C: &str = "ClosedStats:Load";
    let today = now.date_naive();
    let rows = conn
        .query_all(stmt(
            "SELECT d.item_id, d.sub_type, d.day, d.volume, d.median, d.min_price, d.max_price, d.wa_price
             FROM closed_stats_daily d JOIN closed_fetch_state f ON f.item_id = d.item_id
             WHERE f.outcome = 'ok' AND f.fetched_at >= ? AND d.day >= ? AND d.day < ?",
            vec![
                ts(now - Duration::days(FRESH_DAYS)).into(),
                (today - Duration::days(WINDOW_DAYS)).to_string().into(),
                today.to_string().into(),
            ],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            let day: String = r.try_get("", "day").map_err(|e| db_err(C, e))?;
            Ok(ClosedRow {
                item_id: r.try_get("", "item_id").map_err(|e| db_err(C, e))?,
                sub_type: r.try_get("", "sub_type").map_err(|e| db_err(C, e))?,
                day: NaiveDate::parse_from_str(&day, "%Y-%m-%d").map_err(|e| db_err(C, e))?,
                volume: r.try_get("", "volume").map_err(|e| db_err(C, e))?,
                median: r.try_get("", "median").map_err(|e| db_err(C, e))?,
                min_price: r.try_get("", "min_price").map_err(|e| db_err(C, e))?,
                max_price: r.try_get("", "max_price").map_err(|e| db_err(C, e))?,
                wa_price: r.try_get("", "wa_price").map_err(|e| db_err(C, e))?,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(aggregate(rows, today, warm_min_trades))
}

pub const CLOSED_PACE_S: u64 = 10;
pub const IDLE_SLEEP_S: u64 = 60;
pub const CLOSED_RETENTION_DAYS: i64 = 90;
const FAILED_RETRY: i64 = 1; // hours

/// warframe.market publishes yesterday's row shortly after midnight UTC; half past is a safe margin (spec §25 P2).
pub fn cutoff(now: DateTime<Utc>) -> DateTime<Utc> {
    let today = now.date_naive().and_hms_opt(0, 30, 0).expect("valid time").and_utc();
    if now >= today {
        today
    } else {
        today - Duration::days(1)
    }
}

/// Active items that need a fetch: never fetched first, then oldest fetch, then item id (spec §25 P2).
pub async fn stale_items(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<Vec<(String, String)>, Error> {
    const C: &str = "ClosedStats:Stale";
    conn.query_all(stmt(
        "SELECT s.item_id, s.slug FROM sweep_state s LEFT JOIN closed_fetch_state f ON f.item_id = s.item_id
         WHERE s.active = 1 AND (
               f.item_id IS NULL
            OR f.fetched_at < ?
            OR (f.outcome = 'failed' AND f.fetched_at < ?))
         ORDER BY f.fetched_at IS NOT NULL, f.fetched_at, s.item_id",
        vec![ts(cutoff(now)).into(), ts(now - Duration::hours(FAILED_RETRY)).into()],
    ))
    .await
    .map_err(|e| db_err(C, e))?
    .iter()
    .map(|r| Ok((r.try_get("", "item_id").map_err(|e| db_err(C, e))?, r.try_get("", "slug").map_err(|e| db_err(C, e))?)))
    .collect()
}

pub fn pick(stale: &[(String, String)], hot: &HashSet<String>) -> Option<(String, String)> {
    stale.iter().find(|(id, _)| hot.contains(id)).or_else(|| stale.first()).cloned()
}

/// Fetches one stale item through the Cold lane. `None` when nothing is stale; otherwise the stale count before this fetch.
pub async fn refresh_once(
    conn: &DatabaseConnection,
    source: &dyn StatisticsSource,
    limiter: &Limiter,
    hot: &HashSet<String>,
    now: DateTime<Utc>,
) -> Result<Option<usize>, Error> {
    const C: &str = "ClosedStats";
    let stale = stale_items(conn, now).await?;
    let Some((item_id, slug)) = pick(&stale, hot) else { return Ok(None) };
    let outcome = match fetch_with_retries(source, limiter, Lane::Cold, &slug).await {
        Ok(body) => match parse_statistics(&body) {
            Ok(days) => upsert_days(conn, &item_id, &days).await.map(|_| "ok"),
            Err(e) => Err(e),
        },
        Err(FetchError::NotFound) => Ok("missing"),
        Err(e) => Err(Error::new(C, e.to_string(), get_location!())),
    };
    let outcome = outcome.unwrap_or_else(|e| {
        warning(C, format!("{slug}: {}", e.message), &LoggerOptions::default());
        "failed"
    });
    set_fetch_state(conn, &item_id, now, outcome).await?;
    Ok(Some(stale.len()))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RefreshStatus {
    pub active: i64,
    pub ok: i64,
    pub missing: i64,
    pub failed: i64,
    pub stale: i64,
    pub oldest_fetched_at: Option<String>,
}

pub async fn refresh_status(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<RefreshStatus, Error> {
    const C: &str = "ClosedStats:Status";
    let row = conn
        .query_one(stmt(
            "SELECT COUNT(*) AS active,
                    COALESCE(SUM(f.outcome = 'ok'), 0) AS ok,
                    COALESCE(SUM(f.outcome = 'missing'), 0) AS missing,
                    COALESCE(SUM(f.outcome = 'failed'), 0) AS failed,
                    MIN(f.fetched_at) AS oldest
             FROM sweep_state s LEFT JOIN closed_fetch_state f ON f.item_id = s.item_id WHERE s.active = 1",
            vec![],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .ok_or_else(|| db_err(C, "no status row"))?;
    Ok(RefreshStatus {
        active: row.try_get("", "active").map_err(|e| db_err(C, e))?,
        ok: row.try_get("", "ok").map_err(|e| db_err(C, e))?,
        missing: row.try_get("", "missing").map_err(|e| db_err(C, e))?,
        failed: row.try_get("", "failed").map_err(|e| db_err(C, e))?,
        stale: stale_items(conn, now).await?.len() as i64,
        oldest_fetched_at: row.try_get("", "oldest").map_err(|e| db_err(C, e))?,
    })
}

pub async fn apply_retention(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<u64, Error> {
    exec(
        conn,
        "ClosedStats:Retention",
        "DELETE FROM closed_stats_daily WHERE day < ?",
        vec![(now.date_naive() - Duration::days(CLOSED_RETENTION_DAYS)).to_string().into()],
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::backfill::StatsFuture;
    use crate::collector::parse_ts;
    use crate::collector::store::tests::setup;

    const SMALL: &str = include_str!("../../tests/fixtures/statistics_small.json");

    struct Scripted(std::sync::Mutex<std::collections::HashMap<String, std::collections::VecDeque<Result<String, FetchError>>>>);
    impl StatisticsSource for Scripted {
        fn fetch<'a>(&'a self, slug: &'a str) -> StatsFuture<'a> {
            let next = self.0.lock().unwrap().get_mut(slug).and_then(|q| q.pop_front()).unwrap_or(Err(FetchError::Transient("unscripted".into())));
            Box::pin(async move { next })
        }
    }
    fn scripted(entries: Vec<(&str, Vec<Result<String, FetchError>>)>) -> Scripted {
        Scripted(std::sync::Mutex::new(entries.into_iter().map(|(k, v)| (k.to_string(), v.into())).collect()))
    }

    fn day(d: &str) -> NaiveDate {
        NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()
    }
    fn row(d: &str, volume: i64, median: f64, wa: f64) -> ClosedRow {
        ClosedRow { item_id: "item1".into(), sub_type: String::new(), day: day(d), volume, median: Some(median), min_price: Some(median as i64 - 5), max_price: Some(median as i64 + 5), wa_price: Some(wa) }
    }
    fn closed_day(d: &str, volume: i64, median: f64) -> ClosedDay {
        ClosedDay { sub_type: String::new(), day: d.into(), volume, median: Some(median), min_price: Some(1), max_price: Some(99), avg_price: Some(median), wa_price: Some(median) }
    }

    #[test]
    fn aggregate_covers_the_seven_days_before_today_only() {
        // today = 2026-09-20, so W = 09-13 ..= 09-19. 09-12 and 09-20 are outside.
        let rows = vec![
            row("2026-09-12", 100, 500.0, 500.0),
            row("2026-09-13", 10, 60.0, 61.0),
            row("2026-09-15", 20, 64.0, 65.0),
            row("2026-09-16", 5, 66.0, 66.0),
            row("2026-09-18", 30, 70.0, 71.0),
            row("2026-09-19", 10, 68.0, 67.0),
            row("2026-09-20", 100, 500.0, 500.0),
        ];
        let stats = aggregate(rows, day("2026-09-20"), 10);
        assert_eq!(stats.len(), 1);
        let s = &stats[0];
        assert_eq!((s.days, s.trades), (5, 75));
        assert!((s.volume - 75.0 / 7.0).abs() < 1e-9, "trades per day over the whole week, empty days included");
        assert_eq!(s.moving_avg, Some((60.0 + 64.0 + 66.0 + 70.0 + 68.0) / 5.0));
        assert_eq!(s.median, Some(66.0));
        // two most recent days with volume: 09-18 (30 @ 71) and 09-19 (10 @ 67)
        assert_eq!(s.avg_price, Some((30.0 * 71.0 + 10.0 * 67.0) / 40.0));
        assert_eq!((s.min_price, s.max_price), (Some(55), Some(75)));
        assert_eq!(s.week_price_shift, Some(68.0 - 60.0));
        assert!(s.warm);
    }

    #[test]
    fn aggregate_warm_needs_five_days_and_ten_trades_and_shift_needs_two_days() {
        let four_days = vec![row("2026-09-16", 50, 10.0, 10.0), row("2026-09-17", 50, 10.0, 10.0), row("2026-09-18", 50, 10.0, 10.0), row("2026-09-19", 50, 10.0, 10.0)];
        assert!(!aggregate(four_days, day("2026-09-20"), 10)[0].warm, "four days is not enough");
        let thin = (13..=19).map(|d| row(&format!("2026-09-{d}"), 1, 10.0, 10.0)).collect();
        assert!(!aggregate(thin, day("2026-09-20"), 10)[0].warm, "seven trades is not enough");
        let one = aggregate(vec![row("2026-09-19", 3, 10.0, 10.0)], day("2026-09-20"), 10);
        assert_eq!(one[0].week_price_shift, None);
        assert!(aggregate(vec![row("2026-09-01", 3, 10.0, 10.0)], day("2026-09-20"), 10).is_empty(), "no row in W, no stats");
    }

    #[tokio::test]
    async fn upsert_replaces_an_existing_day() {
        let (_dir, conn) = setup().await;
        assert_eq!(upsert_days(&conn, "item1", &[closed_day("2026-09-19", 5, 40.0)]).await.unwrap(), 1);
        upsert_days(&conn, "item1", &[closed_day("2026-09-19", 9, 44.0)]).await.unwrap();
        let kept = conn.query_one(stmt("SELECT volume, median FROM closed_stats_daily WHERE item_id = 'item1' AND day = '2026-09-19'", vec![])).await.unwrap().unwrap();
        assert_eq!(kept.try_get::<i64>("", "volume").unwrap(), 9);
        assert_eq!(kept.try_get::<f64>("", "median").unwrap(), 44.0);
    }

    #[tokio::test]
    async fn load_fresh_needs_an_ok_state_within_three_days() {
        let (_dir, conn) = setup().await;
        let now = parse_ts("2026-09-20T08:00:00Z").unwrap();
        let week: Vec<ClosedDay> = (13..=19).map(|d| closed_day(&format!("2026-09-{d}"), 4, 50.0)).collect();
        upsert_days(&conn, "item1", &week).await.unwrap();
        upsert_days(&conn, "item2", &week).await.unwrap();
        assert!(load_fresh(&conn, now, 10).await.unwrap().is_empty(), "no fetch state yet");
        set_fetch_state(&conn, "item1", now - Duration::days(1), "ok").await.unwrap();
        set_fetch_state(&conn, "item2", now - Duration::days(4), "ok").await.unwrap();
        let fresh = load_fresh(&conn, now, 10).await.unwrap();
        assert_eq!(fresh.iter().map(|s| s.item_id.as_str()).collect::<Vec<_>>(), vec!["item1"], "item2's fetch is four days old");
        assert!(fresh[0].warm);
        set_fetch_state(&conn, "item1", now, "failed").await.unwrap();
        assert!(load_fresh(&conn, now, 10).await.unwrap().is_empty(), "a failed state is not fresh");
    }

    #[test]
    fn aggregate_falls_back_to_the_moving_average_without_a_weighted_price() {
        let bare = |d: &str, volume: i64, median: f64| ClosedRow { wa_price: None, ..row(d, volume, median, 0.0) };
        let stats = aggregate(vec![bare("2026-09-18", 30, 70.0), bare("2026-09-19", 10, 60.0)], day("2026-09-20"), 10);
        assert_eq!(stats[0].avg_price, stats[0].moving_avg, "no day has both volume and a weighted price");
        assert_eq!(stats[0].avg_price, Some(65.0));
    }

    #[test]
    fn cutoff_is_the_most_recent_half_past_midnight_utc() {
        assert_eq!(ts(cutoff(parse_ts("2026-09-20T08:00:00Z").unwrap())), "2026-09-20T00:30:00Z");
        assert_eq!(ts(cutoff(parse_ts("2026-09-20T00:10:00Z").unwrap())), "2026-09-19T00:30:00Z");
    }

    #[tokio::test]
    async fn stale_items_honour_the_cutoff_the_failed_hour_and_inactive_items() {
        let (_dir, conn) = setup().await; // item1/slug1 and item2/slug2, both active
        let now = parse_ts("2026-09-20T08:00:00Z").unwrap();
        assert_eq!(stale_items(&conn, now).await.unwrap().len(), 2, "never fetched");
        set_fetch_state(&conn, "item1", now - Duration::hours(2), "ok").await.unwrap();
        assert_eq!(stale_items(&conn, now).await.unwrap(), vec![("item2".to_string(), "slug2".to_string())], "item1 was fetched after today's cutoff");
        set_fetch_state(&conn, "item1", now - Duration::hours(9), "ok").await.unwrap();
        assert_eq!(stale_items(&conn, now).await.unwrap()[0].0, "item2", "never fetched sorts before an old fetch");
        set_fetch_state(&conn, "item2", now - Duration::minutes(30), "failed").await.unwrap();
        assert_eq!(stale_items(&conn, now).await.unwrap().len(), 1, "a failure is retried only after an hour");
        set_fetch_state(&conn, "item2", now - Duration::minutes(90), "failed").await.unwrap();
        assert_eq!(stale_items(&conn, now).await.unwrap().len(), 2);
        exec(&conn, "Test", "UPDATE sweep_state SET active = 0 WHERE item_id = 'item1'", vec![]).await.unwrap();
        assert_eq!(stale_items(&conn, now).await.unwrap().len(), 1, "inactive items are skipped");
    }

    #[tokio::test]
    async fn a_failure_from_before_the_cutoff_is_stale_even_within_the_hour() {
        let (_dir, conn) = setup().await;
        let now = parse_ts("2026-09-20T01:00:00Z").unwrap(); // cutoff 00:30, the failed hour reaches back to 00:00
        set_fetch_state(&conn, "item1", parse_ts("2026-09-20T00:15:00Z").unwrap(), "failed").await.unwrap();
        set_fetch_state(&conn, "item2", parse_ts("2026-09-20T00:45:00Z").unwrap(), "failed").await.unwrap();
        assert_eq!(
            stale_items(&conn, now).await.unwrap(),
            vec![("item1".to_string(), "slug1".to_string())],
            "item1 failed before today's cutoff; item2 failed after it and is under an hour old"
        );
    }

    #[test]
    fn pick_prefers_a_hot_item() {
        let stale = vec![("a".to_string(), "sa".to_string()), ("b".to_string(), "sb".to_string())];
        assert_eq!(pick(&stale, &HashSet::new()).unwrap().0, "a");
        assert_eq!(pick(&stale, &HashSet::from(["b".to_string()])).unwrap().0, "b");
        assert!(pick(&[], &HashSet::new()).is_none());
    }

    #[tokio::test]
    async fn refresh_once_writes_ok_missing_and_failed_states() {
        let (_dir, conn) = setup().await;
        let now = parse_ts("2026-09-20T08:00:00Z").unwrap();
        let limiter = Limiter::new(1000);
        let source = scripted(vec![
            ("slug1", vec![Ok(SMALL.to_string())]),
            ("slug2", vec![Err(FetchError::NotFound)]),
        ]);
        assert_eq!(refresh_once(&conn, &source, &limiter, &HashSet::new(), now).await.unwrap(), Some(2));
        assert_eq!(refresh_once(&conn, &source, &limiter, &HashSet::new(), now).await.unwrap(), Some(1));
        assert_eq!(refresh_once(&conn, &source, &limiter, &HashSet::new(), now).await.unwrap(), None, "both were fetched after the cutoff");
        let status = refresh_status(&conn, now).await.unwrap();
        assert_eq!((status.active, status.ok, status.missing, status.failed, status.stale), (2, 1, 1, 0, 0));
        let days: i64 = conn.query_one(stmt("SELECT COUNT(*) AS n FROM closed_stats_daily WHERE item_id = 'item1'", vec![])).await.unwrap().unwrap().try_get("", "n").unwrap();
        assert_eq!(days, 4, "the four fixture rows");

        let later = now + Duration::days(1);
        let dead = scripted(vec![("slug1", vec![Err(FetchError::Transient("1".into())), Err(FetchError::Transient("2".into())), Err(FetchError::Transient("3".into()))])]);
        refresh_once(&conn, &dead, &limiter, &HashSet::from(["item1".to_string()]), later).await.unwrap();
        assert_eq!(refresh_status(&conn, later).await.unwrap().failed, 1);
    }

    #[tokio::test]
    async fn retention_drops_days_older_than_ninety() {
        let (_dir, conn) = setup().await;
        upsert_days(&conn, "item1", &[closed_day("2026-06-01", 1, 1.0), closed_day("2026-09-19", 1, 1.0)]).await.unwrap();
        assert_eq!(apply_retention(&conn, parse_ts("2026-09-20T08:00:00Z").unwrap()).await.unwrap(), 1);
    }
}
