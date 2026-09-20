//! warframe.market closed-trade dailies as a price basis (spec §25 P1–P3).

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use service::sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};
use utils::Error;

use super::backfill::ClosedDay;
use super::{db_err, stmt, ts};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;
    use crate::collector::store::tests::setup;

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
}
