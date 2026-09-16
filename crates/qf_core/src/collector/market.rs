//! Market Data aggregates over the collector tables (spec §23 M1–M3).

use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::Error;

use super::stats::ItemStats;
use super::{db_err, stmt};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverviewRow {
    pub item_id: String,
    pub name: String,
    pub slug: String,
    pub sub_type: String,
    pub volume: f64,
    pub avg_price: Option<f64>,
    pub moving_avg: Option<f64>,
    pub median: Option<f64>,
    pub profit: Option<f64>,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
    pub history_days: i64,
    pub warm: bool,
    pub updated_at: String,
}

/// Every stats row with its cache name and slug; rows whose item is not in the cache are skipped (spec §23 M1).
pub fn overview(stats: Vec<ItemStats>, name_of: impl Fn(&str) -> Option<(String, String)>) -> Vec<OverviewRow> {
    stats
        .into_iter()
        .filter_map(|s| {
            let (name, slug) = name_of(&s.item_id)?;
            Some(OverviewRow {
                item_id: s.item_id,
                name,
                slug,
                sub_type: s.sub_type,
                volume: s.volume,
                avg_price: s.avg_price,
                moving_avg: s.moving_avg,
                median: s.median,
                profit: s.profit,
                min_price: s.min_price,
                max_price: s.max_price,
                history_days: s.history_days,
                warm: s.warm,
                updated_at: s.updated_at,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Projection {
    pub date: String,
    pub warm_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistogramBucket {
    pub bucket: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Warmup {
    pub tracked: i64,
    pub warm: i64,
    pub projected: Vec<Projection>,
    pub history_days_histogram: Vec<HistogramBucket>,
    pub trades_histogram: Vec<HistogramBucket>,
}

const WARM_DAYS: i64 = 7;
const WARM_TRADES: f64 = 10.0;

/// Warm counts, a 7-day projection with trade counts held constant, and two histograms (spec §23 M3).
pub fn warmup(stats: &[ItemStats], today: NaiveDate) -> Warmup {
    let trades7 = |s: &ItemStats| (s.volume * 7.0).round();
    let projected = (1..=7)
        .map(|d| Projection {
            date: (today + Duration::days(d)).to_string(),
            warm_count: stats.iter().filter(|s| s.history_days + d >= WARM_DAYS && trades7(s) >= WARM_TRADES).count() as i64,
        })
        .collect();
    let history_days_histogram = (0..=7)
        .map(|day| HistogramBucket {
            bucket: if day == 7 { "7+".into() } else { day.to_string() },
            count: stats.iter().filter(|s| if day == 7 { s.history_days >= 7 } else { s.history_days == day }).count() as i64,
        })
        .collect();
    let trades_histogram = [("0", 0.0, 0.0), ("1-4", 1.0, 4.0), ("5-9", 5.0, 9.0), ("10+", 10.0, f64::INFINITY)]
        .into_iter()
        .map(|(bucket, lo, hi)| HistogramBucket {
            bucket: bucket.into(),
            count: stats.iter().filter(|s| (lo..=hi).contains(&trades7(s))).count() as i64,
        })
        .collect();
    Warmup {
        tracked: stats.len() as i64,
        warm: stats.iter().filter(|s| s.warm).count() as i64,
        projected,
        history_days_histogram,
        trades_histogram,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Mover {
    pub item_id: String,
    pub name: String,
    pub slug: String,
    pub sub_type: String,
    pub median_now: f64,
    pub median_then: f64,
    pub change_pct: f64,
    pub volume: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MoverList {
    pub up: Vec<Mover>,
    pub down: Vec<Mover>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Movers {
    pub day: MoverList,
    pub week: MoverList,
}

const MOVERS_PER_SIDE: usize = 25;

/// Latest daily median against the latest median at or before `days_back` days earlier (spec §23 M2).
async fn movers_for(conn: &DatabaseConnection, days_back: i64, min_volume: f64, name_of: &impl Fn(&str) -> Option<(String, String)>) -> Result<MoverList, Error> {
    const C: &str = "Market:Movers";
    let offset = format!("-{days_back} days");
    let rows = conn
        .query_all(stmt(
            "WITH latest AS (
                SELECT item_id, sub_type, MAX(day) AS day FROM item_stats_daily WHERE median IS NOT NULL GROUP BY item_id, sub_type
             ),
             now AS (
                SELECT d.item_id, d.sub_type, d.day, d.median FROM item_stats_daily d
                JOIN latest l ON l.item_id = d.item_id AND l.sub_type = d.sub_type AND l.day = d.day
             )
             SELECT n.item_id, n.sub_type, n.median AS median_now, s.volume,
                    (SELECT p.median FROM item_stats_daily p
                      WHERE p.item_id = n.item_id AND p.sub_type = n.sub_type AND p.median IS NOT NULL AND p.day <= date(n.day, ?)
                      ORDER BY p.day DESC LIMIT 1) AS median_then
             FROM now n JOIN item_stats s ON s.item_id = n.item_id AND s.sub_type = n.sub_type
             WHERE s.volume >= ?",
            vec![offset.into(), min_volume.into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?;
    let mut all = Vec::new();
    for r in rows.iter() {
        let median_then: Option<f64> = r.try_get("", "median_then").map_err(|e| db_err(C, e))?;
        let Some(median_then) = median_then.filter(|m| *m > 0.0) else { continue };
        let item_id: String = r.try_get("", "item_id").map_err(|e| db_err(C, e))?;
        let Some((name, slug)) = name_of(&item_id) else { continue };
        let median_now: f64 = r.try_get("", "median_now").map_err(|e| db_err(C, e))?;
        all.push(Mover {
            item_id,
            name,
            slug,
            sub_type: r.try_get("", "sub_type").map_err(|e| db_err(C, e))?,
            median_now,
            median_then,
            change_pct: (median_now - median_then) / median_then * 100.0,
            volume: r.try_get("", "volume").map_err(|e| db_err(C, e))?,
        });
    }
    all.sort_by(|a, b| b.change_pct.partial_cmp(&a.change_pct).unwrap_or(std::cmp::Ordering::Equal));
    let up = all.iter().filter(|m| m.change_pct > 0.0).take(MOVERS_PER_SIDE).cloned().collect();
    let down = all.iter().rev().filter(|m| m.change_pct < 0.0).take(MOVERS_PER_SIDE).cloned().collect();
    Ok(MoverList { up, down })
}

/// Top risers and fallers over one and seven days (spec §23 M2).
pub async fn movers(conn: &DatabaseConnection, min_volume: f64, name_of: impl Fn(&str) -> Option<(String, String)>) -> Result<Movers, Error> {
    Ok(Movers { day: movers_for(conn, 1, min_volume, &name_of).await?, week: movers_for(conn, 7, min_volume, &name_of).await? })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::store::{exec, tests::setup};

    async fn daily(conn: &DatabaseConnection, item: &str, day: &str, median: Option<f64>) {
        exec(conn, "Test:Daily", "INSERT INTO item_stats_daily (item_id, sub_type, day, volume, median, min_price, max_price) VALUES (?, '', ?, 5, ?, NULL, NULL)", vec![item.into(), day.into(), median.into()]).await.unwrap();
    }

    async fn current(conn: &DatabaseConnection, item: &str, volume: f64) {
        exec(conn, "Test:Stats", "INSERT INTO item_stats (item_id, sub_type, volume, history_days, warm, updated_at) VALUES (?, '', ?, 3, 0, '2026-09-16T00:00:00Z')", vec![item.into(), volume.into()]).await.unwrap();
    }

    fn stat(id: &str, volume: f64, history_days: i64, warm: bool) -> ItemStats {
        ItemStats {
            item_id: id.into(), sub_type: String::new(), volume, avg_price: Some(10.0), moving_avg: Some(10.0), profit: Some(2.0),
            min_price: Some(8), max_price: Some(12), median: Some(10.0), history_days, warm, updated_at: "2026-09-16T00:00:00Z".into(),
        }
    }

    #[test]
    fn overview_joins_names_and_skips_items_missing_from_the_cache() {
        let rows = overview(vec![stat("a", 1.0, 3, false), stat("gone", 1.0, 3, false)], |id| (id == "a").then(|| ("Item A".to_string(), "item_a".to_string())));
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].name.as_str(), rows[0].slug.as_str(), rows[0].history_days, rows[0].warm), ("Item A", "item_a", 3, false));
    }

    #[test]
    fn warmup_counts_projects_and_buckets() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 16).unwrap();
        let stats = vec![
            stat("warm", 2.0, 9, true),      // 14 trades, already warm
            stat("soon", 12.0 / 7.0, 5, false), // 12 trades, warm when history_days reaches 7: d = 2 → 2026-09-18
            stat("thin", 4.0 / 7.0, 5, false),  // 4 trades: never in this projection
            stat("late", 10.0 / 7.0, 1, false), // 10 trades, warm at d = 6 → 2026-09-22
        ];
        let w = warmup(&stats, today);
        assert_eq!((w.tracked, w.warm), (4, 1));
        assert_eq!(w.projected.len(), 7);
        assert_eq!(w.projected[0], Projection { date: "2026-09-17".into(), warm_count: 1 });
        assert_eq!(w.projected[1], Projection { date: "2026-09-18".into(), warm_count: 2 });
        assert_eq!(w.projected[5], Projection { date: "2026-09-22".into(), warm_count: 3 });
        assert_eq!(w.projected[6].warm_count, 3, "thin never qualifies");
        assert_eq!(
            w.history_days_histogram.iter().filter(|b| b.count > 0).map(|b| (b.bucket.as_str(), b.count)).collect::<Vec<_>>(),
            vec![("1", 1), ("5", 2), ("7+", 1)]
        );
        assert_eq!(
            w.trades_histogram,
            vec![
                HistogramBucket { bucket: "0".into(), count: 0 },
                HistogramBucket { bucket: "1-4".into(), count: 1 },
                HistogramBucket { bucket: "5-9".into(), count: 0 },
                HistogramBucket { bucket: "10+".into(), count: 3 },
            ]
        );
    }

    #[tokio::test]
    async fn movers_compare_the_latest_median_with_one_and_seven_days_earlier_and_drop_thin_items() {
        let (_dir, conn) = setup().await;
        // "rise": 100 → 110 (day) and 80 → 110 (week). The 09-15 row has a NULL median, so the day comparison falls back to 09-14.
        for (day, median) in [("2026-09-08", Some(80.0)), ("2026-09-14", Some(100.0)), ("2026-09-15", None), ("2026-09-16", Some(110.0))] {
            daily(&conn, "rise", day, median).await;
        }
        current(&conn, "rise", 5.0).await;
        // "fall": 50 → 40 over a day; only two days of history, so no week comparison.
        daily(&conn, "fall", "2026-09-15", Some(50.0)).await;
        daily(&conn, "fall", "2026-09-16", Some(40.0)).await;
        current(&conn, "fall", 5.0).await;
        // "thin": huge move but volume below the threshold.
        daily(&conn, "thin", "2026-09-15", Some(10.0)).await;
        daily(&conn, "thin", "2026-09-16", Some(30.0)).await;
        current(&conn, "thin", 1.0).await;

        let m = movers(&conn, 3.0, |id| Some((id.to_uppercase(), id.to_string()))).await.unwrap();
        assert_eq!(m.day.up.iter().map(|x| x.item_id.as_str()).collect::<Vec<_>>(), vec!["rise"]);
        assert!((m.day.up[0].change_pct - 10.0).abs() < 1e-9);
        assert_eq!((m.day.up[0].median_then, m.day.up[0].name.as_str()), (100.0, "RISE"));
        assert_eq!(m.day.down.iter().map(|x| x.item_id.as_str()).collect::<Vec<_>>(), vec!["fall"]);
        assert!((m.day.down[0].change_pct + 20.0).abs() < 1e-9);
        assert_eq!(m.week.up.iter().map(|x| x.item_id.as_str()).collect::<Vec<_>>(), vec!["rise"]);
        assert!((m.week.up[0].change_pct - 37.5).abs() < 1e-9);
        assert!(m.week.down.is_empty(), "fall has no row seven days back");
    }
}
