//! SQL for the analytics tabs (spec §22 A2, A4, A5).

use chrono::{DateTime, Utc};
use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::Error;

use super::{Bucket, ItemRow, PartnerRow, StockRow, TimelineRow};
use crate::collector::{db_err, stmt};
use crate::trader::price_source::PriceSource;

const RANGE: &str = "julianday(created_at) >= julianday(?) AND julianday(created_at) < julianday(?)";

/// One row per item and variant with a transaction in range, profit descending (spec §22 A2).
pub async fn items(conn: &DatabaseConnection, from: &str, to: &str) -> Result<Vec<ItemRow>, Error> {
    const C: &str = "Analytics:Items";
    let sql = format!(
        "SELECT t.wfm_id, t.wfm_url, t.item_name, COALESCE(t.sub_type, '') AS sub_type,
                SUM(t.transaction_type = 'purchase') AS purchases,
                SUM(CASE WHEN t.transaction_type = 'purchase' THEN t.quantity ELSE 0 END) AS bought_qty,
                SUM(CASE WHEN t.transaction_type = 'purchase' THEN t.price ELSE 0 END) AS spend,
                SUM(t.transaction_type = 'sale') AS sales,
                SUM(CASE WHEN t.transaction_type = 'sale' THEN t.quantity ELSE 0 END) AS sold_qty,
                SUM(CASE WHEN t.transaction_type = 'sale' THEN t.price ELSE 0 END) AS revenue,
                SUM(CASE WHEN t.transaction_type = 'sale' THEN COALESCE(t.profit, 0) ELSE 0 END) AS profit,
                AVG(CASE WHEN t.transaction_type = 'sale' THEN
                    (SELECT julianday(t.created_at) - MAX(julianday(p.created_at)) FROM \"transaction\" p
                      WHERE p.wfm_url = t.wfm_url AND COALESCE(p.sub_type, '') = COALESCE(t.sub_type, '')
                        AND p.transaction_type = 'purchase' AND julianday(p.created_at) <= julianday(t.created_at))
                    END) AS avg_days_held
         FROM \"transaction\" t
         WHERE {RANGE}
         GROUP BY t.wfm_url, COALESCE(t.sub_type, '')
         ORDER BY profit DESC, t.item_name"
    );
    conn.query_all(stmt(&sql, vec![from.into(), to.into()]))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            let bought_qty: i64 = r.try_get("", "bought_qty").map_err(|e| db_err(C, e))?;
            let sold_qty: i64 = r.try_get("", "sold_qty").map_err(|e| db_err(C, e))?;
            let spend: i64 = r.try_get("", "spend").map_err(|e| db_err(C, e))?;
            let revenue: i64 = r.try_get("", "revenue").map_err(|e| db_err(C, e))?;
            Ok(ItemRow {
                wfm_id: r.try_get("", "wfm_id").map_err(|e| db_err(C, e))?,
                wfm_url: r.try_get("", "wfm_url").map_err(|e| db_err(C, e))?,
                item_name: r.try_get("", "item_name").map_err(|e| db_err(C, e))?,
                sub_type: r.try_get("", "sub_type").map_err(|e| db_err(C, e))?,
                purchases: r.try_get("", "purchases").map_err(|e| db_err(C, e))?,
                bought_qty,
                spend,
                sales: r.try_get("", "sales").map_err(|e| db_err(C, e))?,
                sold_qty,
                revenue,
                profit: r.try_get("", "profit").map_err(|e| db_err(C, e))?,
                avg_buy: (bought_qty > 0).then(|| spend as f64 / bought_qty as f64),
                avg_sell: (sold_qty > 0).then(|| revenue as f64 / sold_qty as f64),
                avg_days_held: r.try_get("", "avg_days_held").map_err(|e| db_err(C, e))?,
            })
        })
        .collect()
}

/// One row per non-empty trading partner, most trades first (spec §22 A4).
pub async fn partners(conn: &DatabaseConnection, from: &str, to: &str) -> Result<Vec<PartnerRow>, Error> {
    const C: &str = "Analytics:Partners";
    let sql = format!(
        "SELECT user_name, COUNT(*) AS trades,
                SUM(transaction_type = 'purchase') AS bought_count,
                SUM(CASE WHEN transaction_type = 'purchase' THEN price ELSE 0 END) AS bought_plat,
                SUM(transaction_type = 'sale') AS sold_count,
                SUM(CASE WHEN transaction_type = 'sale' THEN price ELSE 0 END) AS sold_plat,
                SUM(CASE WHEN transaction_type = 'sale' THEN COALESCE(profit, 0) ELSE 0 END) AS profit,
                strftime('%Y-%m-%dT%H:%M:%SZ', MAX(julianday(created_at))) AS last_trade_at
         FROM \"transaction\"
         WHERE user_name <> '' AND {RANGE}
         GROUP BY user_name
         ORDER BY trades DESC, user_name"
    );
    conn.query_all(stmt(&sql, vec![from.into(), to.into()]))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            Ok(PartnerRow {
                user_name: r.try_get("", "user_name").map_err(|e| db_err(C, e))?,
                trades: r.try_get("", "trades").map_err(|e| db_err(C, e))?,
                bought_count: r.try_get("", "bought_count").map_err(|e| db_err(C, e))?,
                bought_plat: r.try_get("", "bought_plat").map_err(|e| db_err(C, e))?,
                sold_count: r.try_get("", "sold_count").map_err(|e| db_err(C, e))?,
                sold_plat: r.try_get("", "sold_plat").map_err(|e| db_err(C, e))?,
                profit: r.try_get("", "profit").map_err(|e| db_err(C, e))?,
                last_trade_at: r.try_get("", "last_trade_at").map_err(|e| db_err(C, e))?,
            })
        })
        .collect()
}

/// Per-day or per-ISO-week totals with a running profit total (spec §22 A5).
pub async fn timeline(conn: &DatabaseConnection, from: &str, to: &str, bucket: Bucket) -> Result<Vec<TimelineRow>, Error> {
    const C: &str = "Analytics:Timeline";
    // 'weekday 1' after stepping back six days lands on the Monday of the row's ISO week.
    let start = match bucket {
        Bucket::Day => "date(created_at)",
        Bucket::Week => "date(created_at, '-6 days', 'weekday 1')",
    };
    let sql = format!(
        "SELECT {start} AS bucket_start,
                SUM(transaction_type = 'sale') AS sales,
                SUM(transaction_type = 'purchase') AS purchases,
                SUM(CASE WHEN transaction_type = 'sale' THEN price ELSE 0 END) AS revenue,
                SUM(CASE WHEN transaction_type = 'purchase' THEN price ELSE 0 END) AS expenses,
                SUM(CASE WHEN transaction_type = 'sale' THEN COALESCE(profit, 0) ELSE 0 END) AS profit
         FROM \"transaction\"
         WHERE {RANGE}
         GROUP BY bucket_start
         ORDER BY bucket_start"
    );
    let mut cumulative = 0;
    conn.query_all(stmt(&sql, vec![from.into(), to.into()]))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            let profit: i64 = r.try_get("", "profit").map_err(|e| db_err(C, e))?;
            cumulative += profit;
            Ok(TimelineRow {
                bucket_start: r.try_get("", "bucket_start").map_err(|e| db_err(C, e))?,
                sales: r.try_get("", "sales").map_err(|e| db_err(C, e))?,
                purchases: r.try_get("", "purchases").map_err(|e| db_err(C, e))?,
                revenue: r.try_get("", "revenue").map_err(|e| db_err(C, e))?,
                expenses: r.try_get("", "expenses").map_err(|e| db_err(C, e))?,
                profit,
                cumulative_profit: cumulative,
            })
        })
        .collect()
}

/// Every owned stock row with the trader's view of its market (spec §22 A3), unrealised profit descending, unknown market last.
pub async fn stock(conn: &DatabaseConnection, prices: &dyn PriceSource, now: DateTime<Utc>) -> Result<Vec<StockRow>, Error> {
    const C: &str = "Analytics:Stock";
    let mut rows = conn
        .query_all(stmt(
            "SELECT id, wfm_id, wfm_url, item_name, COALESCE(sub_type, '') AS sub_type, owned, bought, list_price, status, created_at,
                    julianday(?) - julianday(created_at) AS days_in_stock
             FROM stock_item WHERE owned > 0",
            vec![crate::collector::ts(now).into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            let sub_type: String = r.try_get("", "sub_type").map_err(|e| db_err(C, e))?;
            let wfm_id: String = r.try_get("", "wfm_id").map_err(|e| db_err(C, e))?;
            let owned: i64 = r.try_get("", "owned").map_err(|e| db_err(C, e))?;
            let bought: i64 = r.try_get("", "bought").map_err(|e| db_err(C, e))?;
            let list_price: Option<i64> = r.try_get("", "list_price").map_err(|e| db_err(C, e))?;
            // The stored sub_type is the entity's JSON; the price source keys on the same entity type the trader uses.
            let parsed: Option<utils::SubType> = if sub_type.is_empty() { None } else { serde_json::from_str(&sub_type).ok() };
            let info = prices.find_by(&wfm_id, &parsed);
            let median = info.as_ref().map(|i| i.median);
            Ok(StockRow {
                id: r.try_get("", "id").map_err(|e| db_err(C, e))?,
                wfm_id,
                wfm_url: r.try_get("", "wfm_url").map_err(|e| db_err(C, e))?,
                item_name: r.try_get("", "item_name").map_err(|e| db_err(C, e))?,
                sub_type,
                owned,
                bought,
                list_price,
                status: r.try_get("", "status").map_err(|e| db_err(C, e))?,
                created_at: r.try_get("", "created_at").map_err(|e| db_err(C, e))?,
                days_in_stock: r.try_get("", "days_in_stock").map_err(|e| db_err(C, e))?,
                median,
                moving_avg: info.as_ref().and_then(|i| i.moving_avg),
                volume: info.as_ref().map(|i| i.volume),
                warm: info.as_ref().is_some_and(|i| i.warm),
                unrealised: median.map(|m| (m - bought as f64) * owned as f64),
                list_vs_median: median.and_then(|m| list_price.map(|l| l as f64 - m)),
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    rows.sort_by(|a, b| match (a.unrealised, b.unrealised) {
        (Some(x), Some(y)) => y.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.item_name.cmp(&b.item_name),
    });
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::store::exec;

    async fn db() -> (tempfile::TempDir, DatabaseConnection) {
        crate::trader::store::tests::db().await
    }

    /// Inserts one transaction row the way the desktop import and the sale handler store them.
    #[allow(clippy::too_many_arguments)]
    async fn tx(
        conn: &DatabaseConnection,
        kind: &str,
        url: &str,
        sub_type: Option<&str>,
        qty: i64,
        price: i64,
        profit: Option<i64>,
        user: &str,
        at: &str,
    ) {
        exec(
            conn,
            "Test:Tx",
            "INSERT INTO \"transaction\" (wfm_id, wfm_url, item_name, item_type, item_unique_name, sub_type, tags, transaction_type, quantity, user_name, price, profit, credits, created_at, updated_at)
             VALUES (?, ?, ?, 'item', 'N/A', ?, '', ?, ?, ?, ?, ?, 0, ?, ?)",
            vec![
                format!("id-{url}").into(),
                url.into(),
                url.replace('_', " ").into(),
                sub_type.into(),
                kind.into(),
                qty.into(),
                user.into(),
                price.into(),
                profit.into(),
                at.into(),
                at.into(),
            ],
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn items_aggregate_per_item_and_measure_days_held_from_the_latest_prior_purchase() {
        let (_dir, conn) = db().await;
        // Bought 2 at 50 each on the 1st (price is the row total), 1 at 36 on the 5th, sold 1 for 60 on the 8th (profit 24 written at sale time).
        tx(&conn, "purchase", "galvanized_shot", Some("{\"rank\":0}"), 2, 100, None, "alice", "2026-09-01T10:00:00.000000000+00:00").await;
        tx(&conn, "purchase", "galvanized_shot", Some("{\"rank\":0}"), 1, 36, None, "bob", "2026-09-05T10:00:00.000000000+00:00").await;
        // The sea-orm shape sorts below the T-shaped row above as text, but it is the later instant.
        tx(&conn, "purchase", "galvanized_shot", Some("{\"rank\":0}"), 1, 44, None, "bob", "2026-09-05 22:00:00.000000 +00:00").await;
        tx(&conn, "sale", "galvanized_shot", Some("{\"rank\":0}"), 1, 60, Some(24), "carol", "2026-09-08T10:00:00.000000000+00:00").await;
        // A different variant of the same item is its own row.
        tx(&conn, "sale", "galvanized_shot", Some("{\"rank\":5}"), 1, 90, Some(40), "dave", "2026-09-08T11:00:00.000000000+00:00").await;
        // Outside the range (to is exclusive).
        tx(&conn, "sale", "galvanized_shot", Some("{\"rank\":0}"), 1, 999, Some(999), "eve", "2026-09-10 00:00:00.000000 +00:00").await;

        let rows = items(&conn, "2026-09-01", "2026-09-10").await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].sub_type, "{\"rank\":5}", "sorted by profit desc");
        let shot = &rows[1];
        assert_eq!((shot.purchases, shot.bought_qty, shot.spend), (3, 4, 180));
        assert_eq!((shot.sales, shot.sold_qty, shot.revenue, shot.profit), (1, 1, 60, 24));
        assert!((shot.avg_buy.unwrap() - 180.0 / 4.0).abs() < 1e-9);
        assert_eq!(shot.avg_sell, Some(60.0));
        assert!((shot.avg_days_held.unwrap() - 2.5).abs() < 1e-6, "sale on the 8th at 10:00, latest prior purchase on the 5th at 22:00");
        assert_eq!(rows[0].avg_days_held, None, "no purchase for rank 5");
    }

    #[tokio::test]
    async fn partners_split_directions_and_skip_empty_names() {
        let (_dir, conn) = db().await;
        tx(&conn, "purchase", "a", None, 1, 10, None, "alice", "2026-09-01T10:00:00.000000000+00:00").await;
        // The sea-orm shape sorts below the purchase above as text, so only julianday makes it the last trade.
        tx(&conn, "sale", "b", None, 2, 50, Some(20), "alice", "2026-09-03 10:00:00.000000 +00:00").await;
        tx(&conn, "sale", "c", None, 1, 5, Some(1), "", "2026-09-03T10:00:00.000000000+00:00").await;
        let rows = partners(&conn, "2026-09-01", "2026-09-10").await.unwrap();
        assert_eq!(
            rows,
            vec![PartnerRow { user_name: "alice".into(), trades: 2, bought_count: 1, bought_plat: 10, sold_count: 1, sold_plat: 50, profit: 20, last_trade_at: "2026-09-03T10:00:00Z".into() }]
        );
    }

    #[tokio::test]
    async fn timeline_buckets_by_day_or_iso_week_with_a_running_total() {
        let (_dir, conn) = db().await;
        // Wed 2026-09-02 and Sun 2026-09-06 are the same ISO week (Mon 08-31); Mon 2026-09-07 starts the next.
        tx(&conn, "sale", "a", None, 1, 30, Some(10), "u", "2026-09-02T10:00:00.000000000+00:00").await;
        tx(&conn, "purchase", "b", None, 1, 20, None, "u", "2026-09-06T10:00:00.000000000+00:00").await;
        tx(&conn, "sale", "c", None, 1, 40, Some(15), "u", "2026-09-07T10:00:00.000000000+00:00").await;

        let days = timeline(&conn, "2026-09-01", "2026-09-10", Bucket::Day).await.unwrap();
        assert_eq!(days.iter().map(|r| r.bucket_start.as_str()).collect::<Vec<_>>(), vec!["2026-09-02", "2026-09-06", "2026-09-07"]);
        assert_eq!(days.iter().map(|r| r.cumulative_profit).collect::<Vec<_>>(), vec![10, 10, 25]);
        assert_eq!((days[1].purchases, days[1].expenses, days[1].profit), (1, 20, 0));

        let weeks = timeline(&conn, "2026-09-01", "2026-09-10", Bucket::Week).await.unwrap();
        assert_eq!(
            weeks,
            vec![
                TimelineRow { bucket_start: "2026-08-31".into(), sales: 1, purchases: 1, revenue: 30, expenses: 20, profit: 10, cumulative_profit: 10 },
                TimelineRow { bucket_start: "2026-09-07".into(), sales: 1, purchases: 0, revenue: 40, expenses: 0, profit: 15, cumulative_profit: 25 },
            ]
        );
    }

    #[tokio::test]
    async fn stock_joins_market_stats_through_the_trader_price_source_and_sorts_by_unrealised() {
        use crate::collector::stats::ItemStats;
        use crate::trader::price_source::StatsPriceSource;
        let (_dir, conn) = db().await;
        for (name, sub_type, owned, bought, list) in [("a", Some("{\"rank\":0}"), 2, 40, Some(55)), ("b", None, 1, 100, None), ("gone", None, 0, 1, None)] {
            exec(
                &conn,
                "Test:Stock",
                "INSERT INTO stock_item (wfm_id, wfm_url, item_name, item_unique_name, sub_type, bought, list_price, owned, is_hidden, created_at, updated_at, status, price_history, properties)
                 VALUES (?, ?, ?, 'N/A', ?, ?, ?, ?, 0, '2026-09-01T00:00:00.000000000+00:00', '2026-09-01T00:00:00.000000000+00:00', 'pending', '[]', '{}')",
                vec![format!("id-{name}").into(), name.into(), name.into(), sub_type.into(), bought.into(), list.into(), owned.into()],
            )
            .await
            .unwrap();
        }
        let stats = vec![ItemStats {
            item_id: "id-a".into(), sub_type: "rank=0".into(), volume: 2.5, avg_price: Some(50.0), moving_avg: Some(48.0), profit: Some(5.0),
            min_price: Some(40), max_price: Some(60), median: Some(50.0), history_days: 8, warm: true, updated_at: "2026-09-10T00:00:00Z".into(),
        }];
        let prices = StatsPriceSource::from_stats(stats, |id| Some(id.trim_start_matches("id-").to_string()));
        let now = crate::collector::parse_ts("2026-09-11T00:00:00Z").unwrap();

        let rows = stock(&conn, &prices, now).await.unwrap();
        assert_eq!(rows.iter().map(|r| r.wfm_url.as_str()).collect::<Vec<_>>(), vec!["a", "b"], "owned = 0 rows are skipped; known stats first");
        let a = &rows[0];
        assert_eq!((a.owned, a.bought, a.list_price, a.warm), (2, 40, Some(55), true));
        assert_eq!((a.median, a.moving_avg, a.volume), (Some(50.0), Some(48.0), Some(2.5)));
        assert_eq!(a.unrealised, Some(20.0), "(50 - 40) * 2");
        assert_eq!(a.list_vs_median, Some(5.0));
        assert!((a.days_in_stock - 10.0).abs() < 1e-6);
        let b = &rows[1];
        assert_eq!((b.median, b.unrealised, b.list_vs_median, b.warm), (None, None, None, false));
    }
}
