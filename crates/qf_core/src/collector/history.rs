use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::Error;

use super::stats::ItemStats;
use super::{db_err, stmt, ts};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HourlyPoint {
    pub hour: String,
    pub min_sell_min: Option<i64>,
    pub min_sell_avg: Option<f64>,
    pub min_sell_max: Option<i64>,
    pub max_buy_min: Option<i64>,
    pub max_buy_avg: Option<f64>,
    pub max_buy_max: Option<i64>,
    pub sell_count_avg: f64,
    pub buy_count_avg: f64,
    pub samples: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DailyPoint {
    pub day: String,
    pub volume: i64,
    pub median: Option<f64>,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct History {
    pub item_id: String,
    pub sub_types: Vec<String>,
    pub sub_type: String,
    pub stats: Option<ItemStats>,
    pub hourly: Vec<HourlyPoint>,
    pub daily: Vec<DailyPoint>,
    pub last_swept_at: Option<String>,
}

pub async fn load_history(
    conn: &DatabaseConnection,
    item_id: &str,
    sub_type: Option<String>,
    days: i64,
    now: DateTime<Utc>,
) -> Result<History, Error> {
    const C: &str = "Collector:History";
    let sub_types: Vec<String> = conn
        .query_all(stmt(
            "SELECT sub_type FROM item_stats WHERE item_id = ?
             UNION SELECT sub_type FROM sweep_summary_hourly WHERE item_id = ?
             ORDER BY 1",
            vec![item_id.into(), item_id.into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| r.try_get("", "sub_type").map_err(|e| db_err(C, e)))
        .collect::<Result<_, Error>>()?;
    let chosen = sub_type
        .filter(|s| sub_types.contains(s))
        .or_else(|| sub_types.first().cloned())
        .unwrap_or_default();

    let stats = conn
        .query_one(stmt(
            "SELECT item_id, sub_type, volume, avg_price, moving_avg, profit, min_price, max_price, median, history_days, warm, updated_at
             FROM item_stats WHERE item_id = ? AND sub_type = ?",
            vec![item_id.into(), chosen.clone().into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .map(|r| -> Result<ItemStats, Error> {
            Ok(ItemStats {
                item_id: r.try_get("", "item_id").map_err(|e| db_err(C, e))?,
                sub_type: r.try_get("", "sub_type").map_err(|e| db_err(C, e))?,
                volume: r.try_get("", "volume").map_err(|e| db_err(C, e))?,
                avg_price: r.try_get("", "avg_price").map_err(|e| db_err(C, e))?,
                moving_avg: r.try_get("", "moving_avg").map_err(|e| db_err(C, e))?,
                profit: r.try_get("", "profit").map_err(|e| db_err(C, e))?,
                min_price: r.try_get("", "min_price").map_err(|e| db_err(C, e))?,
                max_price: r.try_get("", "max_price").map_err(|e| db_err(C, e))?,
                median: r.try_get("", "median").map_err(|e| db_err(C, e))?,
                history_days: r.try_get("", "history_days").map_err(|e| db_err(C, e))?,
                warm: r.try_get::<i64>("", "warm").map_err(|e| db_err(C, e))? != 0,
                updated_at: r.try_get("", "updated_at").map_err(|e| db_err(C, e))?,
            })
        })
        .transpose()?;

    let since = now - Duration::days(days);
    let hourly = conn
        .query_all(stmt(
            "SELECT hour, min_sell_min, min_sell_avg, min_sell_max, max_buy_min, max_buy_avg, max_buy_max,
                    sell_count_avg, buy_count_avg, samples
             FROM sweep_summary_hourly WHERE item_id = ? AND sub_type = ? AND hour >= ? ORDER BY hour",
            vec![item_id.into(), chosen.clone().into(), ts(since).into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            Ok(HourlyPoint {
                hour: r.try_get("", "hour").map_err(|e| db_err(C, e))?,
                min_sell_min: r.try_get("", "min_sell_min").map_err(|e| db_err(C, e))?,
                min_sell_avg: r.try_get("", "min_sell_avg").map_err(|e| db_err(C, e))?,
                min_sell_max: r.try_get("", "min_sell_max").map_err(|e| db_err(C, e))?,
                max_buy_min: r.try_get("", "max_buy_min").map_err(|e| db_err(C, e))?,
                max_buy_avg: r.try_get("", "max_buy_avg").map_err(|e| db_err(C, e))?,
                max_buy_max: r.try_get("", "max_buy_max").map_err(|e| db_err(C, e))?,
                sell_count_avg: r.try_get("", "sell_count_avg").map_err(|e| db_err(C, e))?,
                buy_count_avg: r.try_get("", "buy_count_avg").map_err(|e| db_err(C, e))?,
                samples: r.try_get("", "samples").map_err(|e| db_err(C, e))?,
            })
        })
        .collect::<Result<_, Error>>()?;

    let daily = conn
        .query_all(stmt(
            "SELECT day, volume, median, min_price, max_price FROM item_stats_daily
             WHERE item_id = ? AND sub_type = ? AND day >= ? ORDER BY day",
            vec![item_id.into(), chosen.clone().into(), since.date_naive().to_string().into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            Ok(DailyPoint {
                day: r.try_get("", "day").map_err(|e| db_err(C, e))?,
                volume: r.try_get("", "volume").map_err(|e| db_err(C, e))?,
                median: r.try_get("", "median").map_err(|e| db_err(C, e))?,
                min_price: r.try_get("", "min_price").map_err(|e| db_err(C, e))?,
                max_price: r.try_get("", "max_price").map_err(|e| db_err(C, e))?,
            })
        })
        .collect::<Result<_, Error>>()?;

    let last_swept_at = conn
        .query_one(stmt("SELECT last_swept_at FROM sweep_state WHERE item_id = ?", vec![item_id.into()]))
        .await
        .map_err(|e| db_err(C, e))?
        .and_then(|r| r.try_get::<Option<String>>("", "last_swept_at").ok().flatten());

    Ok(History { item_id: item_id.to_string(), sub_types, sub_type: chosen, stats, hourly, daily, last_swept_at })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::maintenance::{recompute_item_stats, rollup_daily, rollup_hourly};
    use crate::collector::stats::StatsConfig;
    use crate::collector::store::tests::{at, order, setup, sweep};

    #[tokio::test]
    async fn history_lists_sub_types_and_defaults_to_the_first() {
        let (_dir, conn) = setup().await;
        sweep(&conn, "item1", &[order("s1", "sell", 20, 3, 0, "u1"), order("s2", "sell", 90, 1, 5, "u2")], 0, 300).await;
        sweep(&conn, "item1", &[order("s1", "sell", 20, 1, 0, "u1"), order("s2", "sell", 90, 1, 5, "u2")], 5, 300).await;
        recompute_item_stats(&conn, "item1", at(6), &StatsConfig::default()).await.unwrap();
        rollup_hourly(&conn, at(70)).await.unwrap();
        rollup_daily(&conn, at(70)).await.unwrap();

        let history = load_history(&conn, "item1", None, 7, at(70)).await.unwrap();
        assert_eq!(history.sub_types, vec!["rank=0".to_string(), "rank=5".to_string()]);
        assert_eq!(history.sub_type, "rank=0");
        assert_eq!(history.hourly.len(), 1);
        assert_eq!(history.hourly[0].min_sell_avg, Some(20.0));
        assert_eq!(history.daily.len(), 1);
        assert_eq!(history.stats.as_ref().unwrap().min_price, Some(20));
        assert_eq!(history.last_swept_at.as_deref(), Some("2026-09-15T00:05:00Z"));

        let rank5 = load_history(&conn, "item1", Some("rank=5".into()), 7, at(70)).await.unwrap();
        assert_eq!(rank5.sub_type, "rank=5");
        assert!(rank5.daily.is_empty());

        let unknown = load_history(&conn, "item1", Some("rank=9".into()), 7, at(70)).await.unwrap();
        assert_eq!(unknown.sub_type, "rank=0", "unknown sub-types fall back to the first");
    }
}
