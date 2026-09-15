//! Periodic jobs over collector data: vanish resolution (every 5 min), rollups and retention (hourly).

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, DurationRound, Utc};
use serde::Serialize;
use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::Error;

use super::stats::{compute_item_stats, daily_rollup, resolve_vanish, LiveSpread, StatsConfig, Trade};
use super::store::{count, exec};
use super::{db_err, parse_ts, stmt, ts};

pub const RAW_SUMMARY_RETENTION_DAYS: i64 = 30;
pub const VANISHED_RETENTION_DAYS: i64 = 90;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HourlyReport {
    pub hourly_rows: u64,
    pub daily_rows: u64,
    pub deleted_summaries: u64,
    pub deleted_vanished: u64,
}

/// Resolves `pending` full vanishes whose 2 h window has closed (amendment B8). Returns the affected item ids.
pub async fn resolve_pending(conn: &DatabaseConnection, now: DateTime<Utc>, cfg: &StatsConfig) -> Result<Vec<String>, Error> {
    const C: &str = "Collector:Resolve";
    let rows = conn
        .query_all(stmt(
            "SELECT id, item_id, sub_type, side, user_id, vanished_at FROM vanished_orders
             WHERE status = 'pending' AND vanished_at <= ? ORDER BY id",
            vec![ts(now - cfg.relist_window).into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?;
    let mut items = BTreeSet::new();
    for row in rows {
        let id: i64 = row.try_get("", "id").map_err(|e| db_err(C, e))?;
        let item_id: String = row.try_get("", "item_id").map_err(|e| db_err(C, e))?;
        let sub_type: String = row.try_get("", "sub_type").map_err(|e| db_err(C, e))?;
        let side: String = row.try_get("", "side").map_err(|e| db_err(C, e))?;
        let user_id: String = row.try_get("", "user_id").map_err(|e| db_err(C, e))?;
        let vanished_at: String = row.try_get("", "vanished_at").map_err(|e| db_err(C, e))?;
        let Some(vanished) = parse_ts(&vanished_at) else { continue };

        let half = cfg.bulk_window / 2;
        let bulk = count(
            conn,
            C,
            "SELECT COUNT(*) AS n FROM vanished_orders
             WHERE user_id = ? AND kind = 'full' AND vanished_at >= ? AND vanished_at <= ?",
            vec![user_id.clone().into(), ts(vanished - half).into(), ts(vanished + half).into()],
        )
        .await?;

        let after = ts(vanished);
        let until = ts(vanished + cfg.relist_window);
        let relisted = count(
            conn,
            C,
            "SELECT (EXISTS (SELECT 1 FROM last_seen_orders
                        WHERE user_id = ? AND item_id = ? AND sub_type = ? AND side = ? AND first_seen > ? AND first_seen <= ?)
                  OR EXISTS (SELECT 1 FROM vanished_orders
                        WHERE id != ? AND user_id = ? AND item_id = ? AND sub_type = ? AND side = ? AND first_seen > ? AND first_seen <= ?)) AS n",
            vec![
                user_id.clone().into(),
                item_id.clone().into(),
                sub_type.clone().into(),
                side.clone().into(),
                after.clone().into(),
                until.clone().into(),
                id.into(),
                user_id.into(),
                item_id.clone().into(),
                sub_type.into(),
                side.into(),
                after.into(),
                until.into(),
            ],
        )
        .await?
            > 0;

        let resolution = resolve_vanish(bulk, relisted, cfg);
        exec(conn, C, "UPDATE vanished_orders SET status = ? WHERE id = ?", vec![resolution.as_str().into(), id.into()]).await?;
        items.insert(item_id);
    }
    Ok(items.into_iter().collect())
}

/// Rewrites `item_stats` for every sub-type of the item that has recent trades or appears in its latest sweep.
pub async fn recompute_item_stats(conn: &DatabaseConnection, item_id: &str, now: DateTime<Utc>, cfg: &StatsConfig) -> Result<usize, Error> {
    const C: &str = "Collector:Stats";
    let first_swept_at = conn
        .query_one(stmt("SELECT first_swept_at FROM sweep_state WHERE item_id = ?", vec![item_id.into()]))
        .await
        .map_err(|e| db_err(C, e))?
        .and_then(|r| r.try_get::<Option<String>>("", "first_swept_at").ok().flatten())
        .as_deref()
        .and_then(parse_ts);

    let mut groups: BTreeMap<String, (Vec<Trade>, LiveSpread)> = BTreeMap::new();
    let trades = conn
        .query_all(stmt(
            "SELECT sub_type, side, platinum, vanished_at FROM vanished_orders
             WHERE item_id = ? AND status = 'trade' AND vanished_at > ?",
            vec![item_id.into(), ts(now - cfg.window).into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?;
    for row in trades {
        let vanished_at: String = row.try_get("", "vanished_at").map_err(|e| db_err(C, e))?;
        let Some(at) = parse_ts(&vanished_at) else { continue };
        let sub_type: String = row.try_get("", "sub_type").map_err(|e| db_err(C, e))?;
        groups.entry(sub_type).or_default().0.push(Trade {
            side: row.try_get("", "side").map_err(|e| db_err(C, e))?,
            platinum: row.try_get("", "platinum").map_err(|e| db_err(C, e))?,
            at,
        });
    }
    let spreads = conn
        .query_all(stmt(
            "SELECT sub_type, min_sell, max_buy FROM sweep_summary
             WHERE item_id = ? AND swept_at = (SELECT MAX(swept_at) FROM sweep_summary WHERE item_id = ?)",
            vec![item_id.into(), item_id.into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?;
    for row in spreads {
        let sub_type: String = row.try_get("", "sub_type").map_err(|e| db_err(C, e))?;
        groups.entry(sub_type).or_default().1 = LiveSpread {
            min_sell: row.try_get("", "min_sell").map_err(|e| db_err(C, e))?,
            max_buy: row.try_get("", "max_buy").map_err(|e| db_err(C, e))?,
        };
    }

    for (sub_type, (trades, spread)) in &groups {
        let s = compute_item_stats(item_id, sub_type, trades, spread, first_swept_at, now, cfg);
        exec(
            conn,
            C,
            "INSERT OR REPLACE INTO item_stats
                (item_id, sub_type, volume, avg_price, moving_avg, profit, min_price, max_price, median, history_days, warm, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            vec![
                s.item_id.into(),
                s.sub_type.into(),
                s.volume.into(),
                s.avg_price.into(),
                s.moving_avg.into(),
                s.profit.into(),
                s.min_price.into(),
                s.max_price.into(),
                s.median.into(),
                s.history_days.into(),
                (s.warm as i64).into(),
                s.updated_at.into(),
            ],
        )
        .await?;
    }
    Ok(groups.len())
}

pub async fn resolve_and_recompute(conn: &DatabaseConnection, now: DateTime<Utc>, cfg: &StatsConfig) -> Result<usize, Error> {
    let items = resolve_pending(conn, now, cfg).await?;
    for item_id in &items {
        recompute_item_stats(conn, item_id, now, cfg).await?;
    }
    Ok(items.len())
}

/// Rebuilds `sweep_summary_hourly` for the 48 complete hours before `now`.
pub async fn rollup_hourly(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<u64, Error> {
    const C: &str = "Collector:RollupHourly";
    let to = now.duration_trunc(Duration::hours(1)).map_err(|e| db_err(C, e))?;
    let from = to - Duration::hours(48);
    exec(
        conn,
        C,
        "INSERT OR REPLACE INTO sweep_summary_hourly
            (item_id, sub_type, hour, min_sell_min, min_sell_avg, min_sell_max, max_buy_min, max_buy_avg, max_buy_max,
             sell_count_avg, buy_count_avg, samples)
         SELECT item_id, sub_type, substr(swept_at, 1, 13) || ':00:00Z',
                MIN(min_sell), AVG(min_sell), MAX(min_sell), MIN(max_buy), AVG(max_buy), MAX(max_buy),
                AVG(sell_count), AVG(buy_count), COUNT(*)
         FROM sweep_summary
         WHERE swept_at >= ? AND swept_at < ?
         GROUP BY item_id, sub_type, substr(swept_at, 1, 13)",
        vec![ts(from).into(), ts(to).into()],
    )
    .await
}

/// Rebuilds `item_stats_daily` from trades since the start of the day two days before `now`.
pub async fn rollup_daily(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<u64, Error> {
    const C: &str = "Collector:RollupDaily";
    let from = now.duration_trunc(Duration::days(1)).map_err(|e| db_err(C, e))? - Duration::days(2);
    let rows = conn
        .query_all(stmt(
            "SELECT item_id, sub_type, side, platinum, vanished_at FROM vanished_orders
             WHERE status = 'trade' AND vanished_at >= ?",
            vec![ts(from).into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?;
    let mut groups: BTreeMap<(String, String), Vec<Trade>> = BTreeMap::new();
    for row in rows {
        let vanished_at: String = row.try_get("", "vanished_at").map_err(|e| db_err(C, e))?;
        let Some(at) = parse_ts(&vanished_at) else { continue };
        let key = (
            row.try_get::<String>("", "item_id").map_err(|e| db_err(C, e))?,
            row.try_get::<String>("", "sub_type").map_err(|e| db_err(C, e))?,
        );
        groups.entry(key).or_default().push(Trade {
            side: row.try_get("", "side").map_err(|e| db_err(C, e))?,
            platinum: row.try_get("", "platinum").map_err(|e| db_err(C, e))?,
            at,
        });
    }
    let mut written = 0;
    for ((item_id, sub_type), trades) in groups {
        for day in daily_rollup(&trades) {
            written += exec(
                conn,
                C,
                "INSERT OR REPLACE INTO item_stats_daily (item_id, sub_type, day, volume, median, min_price, max_price)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                vec![
                    item_id.clone().into(),
                    sub_type.clone().into(),
                    day.day.to_string().into(),
                    day.volume.into(),
                    day.median.into(),
                    day.min_price.into(),
                    day.max_price.into(),
                ],
            )
            .await?;
        }
    }
    Ok(written)
}

/// Deletes raw summaries older than 30 days and vanished orders older than 90 days (spec §6).
pub async fn apply_retention(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<(u64, u64), Error> {
    const C: &str = "Collector:Retention";
    let summaries = exec(
        conn,
        C,
        "DELETE FROM sweep_summary WHERE swept_at < ?",
        vec![ts(now - Duration::days(RAW_SUMMARY_RETENTION_DAYS)).into()],
    )
    .await?;
    let vanished = exec(
        conn,
        C,
        "DELETE FROM vanished_orders WHERE vanished_at < ?",
        vec![ts(now - Duration::days(VANISHED_RETENTION_DAYS)).into()],
    )
    .await?;
    Ok((summaries, vanished))
}

pub async fn hourly(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<HourlyReport, Error> {
    let hourly_rows = rollup_hourly(conn, now).await?;
    let daily_rows = rollup_daily(conn, now).await?;
    let (deleted_summaries, deleted_vanished) = apply_retention(conn, now).await?;
    Ok(HourlyReport { hourly_rows, daily_rows, deleted_summaries, deleted_vanished })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::store::tests::{at, order, setup, sweep};

    async fn statuses(conn: &DatabaseConnection) -> Vec<String> {
        conn.query_all(stmt("SELECT status FROM vanished_orders ORDER BY id", vec![]))
            .await
            .unwrap()
            .iter()
            .map(|r| r.try_get("", "status").unwrap())
            .collect()
    }

    #[tokio::test]
    async fn a_vanish_becomes_a_trade_after_the_relist_window() {
        let (_dir, conn) = setup().await;
        let cfg = StatsConfig::default();
        sweep(&conn, "item1", &[order("s1", "sell", 20, 1, 0, "u1")], 0, 300).await;
        sweep(&conn, "item1", &[], 5, 300).await;
        assert!(resolve_pending(&conn, at(124), &cfg).await.unwrap().is_empty());
        assert_eq!(statuses(&conn).await, vec!["pending"]);
        assert_eq!(resolve_pending(&conn, at(126), &cfg).await.unwrap(), vec!["item1".to_string()]);
        assert_eq!(statuses(&conn).await, vec!["trade"]);
    }

    #[tokio::test]
    async fn a_new_order_from_the_same_user_within_two_hours_is_a_relist() {
        let (_dir, conn) = setup().await;
        let cfg = StatsConfig::default();
        sweep(&conn, "item1", &[order("s1", "sell", 20, 1, 0, "u1")], 0, 300).await;
        sweep(&conn, "item1", &[], 5, 300).await;
        sweep(&conn, "item1", &[order("s9", "sell", 19, 1, 0, "u1")], 10, 300).await;
        resolve_pending(&conn, at(130), &cfg).await.unwrap();
        assert_eq!(statuses(&conn).await, vec!["relist"]);
    }

    #[tokio::test]
    async fn a_new_order_on_another_sub_type_is_not_a_relist() {
        let (_dir, conn) = setup().await;
        let cfg = StatsConfig::default();
        sweep(&conn, "item1", &[order("s1", "sell", 20, 1, 0, "u1")], 0, 300).await;
        sweep(&conn, "item1", &[], 5, 300).await;
        sweep(&conn, "item1", &[order("s9", "sell", 90, 1, 5, "u1")], 10, 300).await;
        resolve_pending(&conn, at(130), &cfg).await.unwrap();
        assert_eq!(statuses(&conn).await, vec!["trade"]);
    }

    #[tokio::test]
    async fn three_vanishes_by_one_user_across_items_are_a_bulk_pull() {
        let (_dir, conn) = setup().await;
        let cfg = StatsConfig::default();
        sweep(&conn, "item1", &[order("a", "sell", 20, 1, 0, "u1"), order("b", "sell", 90, 1, 5, "u1")], 0, 300).await;
        sweep(&conn, "item2", &[order("c", "sell", 30, 1, 0, "u1")], 0, 300).await;
        sweep(&conn, "item1", &[], 5, 300).await;
        sweep(&conn, "item2", &[], 10, 300).await;
        let items = resolve_pending(&conn, at(200), &cfg).await.unwrap();
        assert_eq!(items, vec!["item1".to_string(), "item2".to_string()]);
        assert_eq!(statuses(&conn).await, vec!["bulk", "bulk", "bulk"]);
    }

    #[tokio::test]
    async fn gap_vanishes_are_never_resolved() {
        let (_dir, conn) = setup().await;
        let cfg = StatsConfig::default();
        sweep(&conn, "item1", &[order("s1", "sell", 20, 1, 0, "u1")], 0, 300).await;
        sweep(&conn, "item1", &[], 16, 300).await;
        assert!(resolve_pending(&conn, at(300), &cfg).await.unwrap().is_empty());
        assert_eq!(statuses(&conn).await, vec!["gap"]);
    }

    #[tokio::test]
    async fn recompute_writes_one_row_per_sub_type_with_live_spread() {
        let (_dir, conn) = setup().await;
        let cfg = StatsConfig::default();
        let first = [
            order("s1", "sell", 20, 3, 0, "u1"),
            order("s2", "sell", 90, 2, 5, "u2"),
            order("b1", "buy", 15, 1, 0, "u3"),
        ];
        let second = [
            order("s1", "sell", 20, 1, 0, "u1"),
            order("s2", "sell", 90, 1, 5, "u2"),
            order("b1", "buy", 15, 1, 0, "u3"),
        ];
        sweep(&conn, "item1", &first, 0, 300).await;
        sweep(&conn, "item1", &second, 5, 300).await;
        assert_eq!(recompute_item_stats(&conn, "item1", at(6), &cfg).await.unwrap(), 2);

        let rows = conn
            .query_all(stmt("SELECT sub_type, volume, min_price, profit, warm FROM item_stats ORDER BY sub_type", vec![]))
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].try_get::<String>("", "sub_type").unwrap(), "rank=0");
        assert_eq!(rows[0].try_get::<f64>("", "volume").unwrap(), 1.0 / 7.0);
        assert_eq!(rows[0].try_get::<Option<i64>>("", "min_price").unwrap(), Some(20));
        assert_eq!(rows[0].try_get::<Option<f64>>("", "profit").unwrap(), Some(5.0));
        assert_eq!(rows[0].try_get::<i64>("", "warm").unwrap(), 0);
        assert_eq!(rows[1].try_get::<String>("", "sub_type").unwrap(), "rank=5");
        assert_eq!(rows[1].try_get::<Option<f64>>("", "profit").unwrap(), None);
    }

    #[tokio::test]
    async fn hourly_rollup_averages_summaries_in_the_hour() {
        let (_dir, conn) = setup().await;
        sweep(&conn, "item1", &[order("s1", "sell", 20, 1, 0, "u1")], 0, 300).await;
        sweep(&conn, "item1", &[order("s1", "sell", 30, 1, 0, "u1")], 5, 300).await;
        assert_eq!(rollup_hourly(&conn, at(70)).await.unwrap(), 1);
        let row = conn
            .query_one(stmt("SELECT hour, min_sell_avg, samples FROM sweep_summary_hourly", vec![]))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<String>("", "hour").unwrap(), "2026-09-15T00:00:00Z");
        assert_eq!(row.try_get::<f64>("", "min_sell_avg").unwrap(), 25.0);
        assert_eq!(row.try_get::<i64>("", "samples").unwrap(), 2);
        rollup_hourly(&conn, at(70)).await.unwrap();
        assert_eq!(
            count(&conn, "t", "SELECT COUNT(*) AS n FROM sweep_summary_hourly", vec![]).await.unwrap(),
            1,
            "re-running replaces, never duplicates"
        );
    }

    #[tokio::test]
    async fn daily_rollup_counts_trades_per_day() {
        let (_dir, conn) = setup().await;
        sweep(&conn, "item1", &[order("s1", "sell", 20, 3, 0, "u1")], 0, 300).await;
        sweep(&conn, "item1", &[order("s1", "sell", 20, 1, 0, "u1")], 5, 300).await;
        assert_eq!(rollup_daily(&conn, at(60)).await.unwrap(), 1);
        let row = conn
            .query_one(stmt("SELECT day, volume, median FROM item_stats_daily", vec![]))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<String>("", "day").unwrap(), "2026-09-15");
        assert_eq!(row.try_get::<i64>("", "volume").unwrap(), 1);
        assert_eq!(row.try_get::<Option<f64>>("", "median").unwrap(), Some(20.0));
    }

    #[tokio::test]
    async fn retention_removes_old_raw_rows() {
        let (_dir, conn) = setup().await;
        let old = -(91 * 24 * 60);
        sweep(&conn, "item1", &[order("s1", "sell", 20, 1, 0, "u1")], old, 300).await;
        sweep(&conn, "item1", &[], old + 5, 300).await;
        sweep(&conn, "item2", &[order("x1", "sell", 5, 1, 0, "u9")], 0, 300).await;
        assert_eq!(apply_retention(&conn, at(0)).await.unwrap(), (1, 1));
        assert_eq!(count(&conn, "t", "SELECT COUNT(*) AS n FROM sweep_summary", vec![]).await.unwrap(), 1);
        assert_eq!(count(&conn, "t", "SELECT COUNT(*) AS n FROM vanished_orders", vec![]).await.unwrap(), 0);
    }
}
