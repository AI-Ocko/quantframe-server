//! One-off import of warframe.market's 90-day closed-trade statistics (spec §24).

use serde::Deserialize;
use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::{get_location, Error};

use super::orders::sub_type_key;
use super::{db_err, stmt};

#[derive(Debug, Clone, PartialEq)]
pub struct ClosedDay {
    pub sub_type: String,
    pub day: String,
    pub volume: i64,
    pub median: Option<f64>,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
}

#[derive(Deserialize)]
struct Body {
    payload: Payload,
}
#[derive(Deserialize)]
struct Payload {
    statistics_closed: Closed,
}
#[derive(Deserialize)]
struct Closed {
    #[serde(rename = "90days")]
    days90: Vec<Row>,
}
#[derive(Deserialize)]
struct Row {
    datetime: String,
    volume: i64,
    min_price: Option<f64>,
    max_price: Option<f64>,
    median: Option<f64>,
    mod_rank: Option<i64>,
    subtype: Option<String>,
}

/// The `90days` closed-trade series as `item_stats_daily` rows keyed by the collector's sub-type key (spec §24 K1).
pub fn parse_statistics(body: &str) -> Result<Vec<ClosedDay>, Error> {
    const C: &str = "Backfill:Parse";
    let body: Body = serde_json::from_str(body).map_err(|e| Error::new(C, e.to_string(), get_location!()))?;
    Ok(body
        .payload
        .statistics_closed
        .days90
        .into_iter()
        .map(|r| ClosedDay {
            sub_type: sub_type_key(r.mod_rank, None, r.subtype.as_deref(), None, None),
            day: r.datetime.chars().take(10).collect(),
            volume: r.volume,
            median: r.median,
            min_price: r.min_price.map(|p| p.round() as i64),
            max_price: r.max_price.map(|p| p.round() as i64),
        })
        .collect())
}

/// Inserts the days the collector has not produced; existing `(item_id, sub_type, day)` rows are left alone (spec §24 K2).
pub async fn insert_missing(conn: &DatabaseConnection, item_id: &str, days: &[ClosedDay]) -> Result<u64, Error> {
    const C: &str = "Backfill:Insert";
    let mut inserted = 0;
    for d in days {
        let result = conn
            .execute(stmt(
                "INSERT OR IGNORE INTO item_stats_daily (item_id, sub_type, day, volume, median, min_price, max_price) VALUES (?, ?, ?, ?, ?, ?, ?)",
                vec![item_id.into(), d.sub_type.clone().into(), d.day.clone().into(), d.volume.into(), d.median.into(), d.min_price.into(), d.max_price.into()],
            ))
            .await
            .map_err(|e| db_err(C, e))?;
        inserted += result.rows_affected();
    }
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::store::{exec, tests::setup};

    const SMALL: &str = include_str!("../../tests/fixtures/statistics_small.json");

    #[test]
    fn parses_the_90_day_series_with_the_collector_sub_type_keys() {
        let days = parse_statistics(SMALL).unwrap();
        assert_eq!(
            days,
            vec![
                ClosedDay { sub_type: "rank=10".into(), day: "2026-09-14".into(), volume: 39, median: Some(50.0), min_price: Some(47), max_price: Some(50) },
                ClosedDay { sub_type: "rank=0".into(), day: "2026-09-15".into(), volume: 12, median: Some(22.0), min_price: Some(20), max_price: Some(25) },
                ClosedDay { sub_type: "subtype=intact".into(), day: "2026-09-15".into(), volume: 15, median: Some(11.0), min_price: Some(10), max_price: Some(12) },
                ClosedDay { sub_type: String::new(), day: "2026-09-15".into(), volume: 43, median: Some(69.0), min_price: Some(66), max_price: Some(70) },
            ]
        );
    }

    #[test]
    fn rejects_bodies_without_the_series() {
        assert!(parse_statistics("{}").is_err());
        assert!(parse_statistics(r#"{"payload":{"statistics_closed":{"90days":"nope"}}}"#).is_err());
        assert!(parse_statistics("not json").is_err());
        assert!(parse_statistics(r#"{"payload":{"statistics_closed":{"90days":[]}}}"#).unwrap().is_empty());
    }

    #[tokio::test]
    async fn insert_missing_adds_new_days_and_never_overwrites_collector_days() {
        let (_dir, conn) = setup().await;
        exec(&conn, "Test", "INSERT INTO item_stats_daily (item_id, sub_type, day, volume, median, min_price, max_price) VALUES ('item1', 'rank=0', '2026-09-15', 7, 99.0, 90, 100)", vec![]).await.unwrap();
        let days = parse_statistics(SMALL).unwrap();
        let inserted = insert_missing(&conn, "item1", &days).await.unwrap();
        assert_eq!(inserted, 3, "the rank=0 2026-09-15 row already existed");
        let kept = conn
            .query_one(stmt("SELECT volume, median FROM item_stats_daily WHERE item_id = 'item1' AND sub_type = 'rank=0' AND day = '2026-09-15'", vec![]))
            .await.unwrap().unwrap();
        assert_eq!(kept.try_get::<i64>("", "volume").unwrap(), 7);
        assert_eq!(kept.try_get::<f64>("", "median").unwrap(), 99.0);
        assert_eq!(insert_missing(&conn, "item1", &days).await.unwrap(), 0, "idempotent");
    }
}
