//! Market Data aggregates over the collector tables (spec §23 M1–M3).

use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use super::stats::ItemStats;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
