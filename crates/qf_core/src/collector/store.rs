use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use service::sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait, Value};
use utils::Error;

use super::diff::{diff_orders, summarize, to_seen, SeenOrder};
use super::orders::V2Order;
use super::{db_err, parse_ts, stmt, ts};

#[derive(Debug, Clone, PartialEq)]
pub struct SweepTarget {
    pub item_id: String,
    pub slug: String,
    pub last_attempt_at: Option<String>,
}

pub struct SweepInput<'a> {
    pub item_id: &'a str,
    pub lane: &'a str,
    pub orders: &'a [V2Order],
    pub swept_at: DateTime<Utc>,
    pub expected_interval_s: i64,
    pub gap_factor: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SweepOutcome {
    pub orders: usize,
    pub groups: usize,
    pub new: usize,
    pub vanished: usize,
    pub partials: usize,
    pub gap: bool,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct ItemCounts {
    pub active: i64,
    pub inactive: i64,
    pub behind: i64,
}

pub(crate) async fn exec<C: ConnectionTrait>(db: &C, component: &str, sql: &str, values: Vec<Value>) -> Result<u64, Error> {
    db.execute(stmt(sql, values))
        .await
        .map(|r| r.rows_affected())
        .map_err(|e| db_err(component, e))
}

pub(crate) async fn count<C: ConnectionTrait>(db: &C, component: &str, sql: &str, values: Vec<Value>) -> Result<i64, Error> {
    let row = db
        .query_one(stmt(sql, values))
        .await
        .map_err(|e| db_err(component, e))?
        .ok_or_else(|| db_err(component, "COUNT returned no row"))?;
    row.try_get("", "n").map_err(|e| db_err(component, e))
}

/// Makes `sweep_state` match the item list: listed items become active (new ones are added), unlisted ones inactive.
pub async fn sync_items(conn: &DatabaseConnection, items: &[(String, String)]) -> Result<(), Error> {
    const C: &str = "Collector:SyncItems";
    let txn = conn.begin().await.map_err(|e| db_err(C, e))?;
    exec(&txn, C, "UPDATE sweep_state SET active = 0", vec![]).await?;
    for (item_id, slug) in items {
        exec(
            &txn,
            C,
            "INSERT INTO sweep_state (item_id, slug, active) VALUES (?, ?, 1)
             ON CONFLICT(item_id) DO UPDATE SET slug = excluded.slug, active = 1",
            vec![item_id.clone().into(), slug.clone().into()],
        )
        .await?;
    }
    txn.commit().await.map_err(|e| db_err(C, e))
}

/// Hot set for phase 2 (amendment B3): every item in stock or on the wish list.
pub async fn hot_item_ids(conn: &DatabaseConnection) -> Result<HashSet<String>, Error> {
    const C: &str = "Collector:HotSet";
    conn.query_all(stmt("SELECT wfm_id FROM stock_item UNION SELECT wfm_id FROM wish_list", vec![]))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| r.try_get::<String>("", "wfm_id").map_err(|e| db_err(C, e)))
        .collect()
}

async fn read_targets(conn: &DatabaseConnection, sql: &str, values: Vec<Value>) -> Result<Vec<SweepTarget>, Error> {
    const C: &str = "Collector:Targets";
    conn.query_all(stmt(sql, values))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            Ok(SweepTarget {
                item_id: r.try_get("", "item_id").map_err(|e| db_err(C, e))?,
                slug: r.try_get("", "slug").map_err(|e| db_err(C, e))?,
                last_attempt_at: r.try_get("", "last_attempt_at").map_err(|e| db_err(C, e))?,
            })
        })
        .collect()
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(", ")
}

/// Active hot items, least recently attempted first.
pub async fn hot_targets(conn: &DatabaseConnection, hot: &HashSet<String>) -> Result<Vec<SweepTarget>, Error> {
    if hot.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT item_id, slug, last_attempt_at FROM sweep_state
         WHERE active = 1 AND item_id IN ({})
         ORDER BY last_attempt_at IS NOT NULL, last_attempt_at, item_id",
        placeholders(hot.len())
    );
    read_targets(conn, &sql, hot.iter().map(|id| id.clone().into()).collect()).await
}

/// Active items, never attempted first, then least recently attempted.
pub async fn cold_candidates(conn: &DatabaseConnection, limit: i64) -> Result<Vec<SweepTarget>, Error> {
    read_targets(
        conn,
        "SELECT item_id, slug, last_attempt_at FROM sweep_state
         WHERE active = 1
         ORDER BY last_attempt_at IS NOT NULL, last_attempt_at, item_id
         LIMIT ?",
        vec![limit.into()],
    )
    .await
}

pub async fn active_count(conn: &DatabaseConnection) -> Result<i64, Error> {
    count(conn, "Collector:ActiveCount", "SELECT COUNT(*) AS n FROM sweep_state WHERE active = 1", vec![]).await
}

pub async fn mark_attempt(conn: &DatabaseConnection, item_id: &str, at: DateTime<Utc>) -> Result<(), Error> {
    exec(conn, "Collector:MarkAttempt", "UPDATE sweep_state SET last_attempt_at = ? WHERE item_id = ?", vec![ts(at).into(), item_id.into()])
        .await
        .map(|_| ())
}

pub async fn record_error(conn: &DatabaseConnection, item_id: &str) -> Result<(), Error> {
    exec(conn, "Collector:RecordError", "UPDATE sweep_state SET consecutive_errors = consecutive_errors + 1 WHERE item_id = ?", vec![item_id.into()])
        .await
        .map(|_| ())
}

/// After a 404 the item stays inactive until the next item-list sync (spec §8).
pub async fn deactivate(conn: &DatabaseConnection, item_id: &str) -> Result<(), Error> {
    exec(conn, "Collector:Deactivate", "UPDATE sweep_state SET active = 0 WHERE item_id = ?", vec![item_id.into()])
        .await
        .map(|_| ())
}

#[allow(clippy::too_many_arguments)]
async fn insert_vanish<C: ConnectionTrait>(
    db: &C,
    item_id: &str,
    order: &SeenOrder,
    quantity: i64,
    vanished_at: &str,
    gap_seconds: Option<i64>,
    kind: &str,
    status: &str,
) -> Result<(), Error> {
    exec(
        db,
        "Collector:InsertVanish",
        "INSERT INTO vanished_orders
            (order_id, item_id, sub_type, side, platinum, quantity, user_id, first_seen, vanished_at, gap_seconds, kind, status)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        vec![
            order.order_id.clone().into(),
            item_id.into(),
            order.sub_type.clone().into(),
            order.side.clone().into(),
            order.platinum.into(),
            quantity.into(),
            order.user_id.clone().into(),
            order.first_seen.clone().into(),
            vanished_at.into(),
            gap_seconds.into(),
            kind.into(),
            status.into(),
        ],
    )
    .await
    .map(|_| ())
}

/// Writes one sweep in a single transaction (spec §5.4 steps 3–5, amendments B5–B8).
pub async fn apply_sweep(conn: &DatabaseConnection, input: SweepInput<'_>) -> Result<SweepOutcome, Error> {
    const C: &str = "Collector:ApplySweep";
    let swept_at = ts(input.swept_at);
    let txn = conn.begin().await.map_err(|e| db_err(C, e))?;

    let state = txn
        .query_one(stmt("SELECT last_swept_at, expected_interval_s FROM sweep_state WHERE item_id = ?", vec![input.item_id.into()]))
        .await
        .map_err(|e| db_err(C, e))?;
    let (last_swept_at, previous_expected): (Option<String>, i64) = match state {
        Some(row) => (
            row.try_get("", "last_swept_at").map_err(|e| db_err(C, e))?,
            row.try_get("", "expected_interval_s").map_err(|e| db_err(C, e))?,
        ),
        None => (None, 0),
    };
    let gap_seconds = last_swept_at.as_deref().and_then(parse_ts).map(|t| (input.swept_at - t).num_seconds());
    let allowed = input.gap_factor * previous_expected.max(input.expected_interval_s) as f64;
    let gap = gap_seconds.is_some_and(|g| g as f64 > allowed);

    let previous: Vec<SeenOrder> = txn
        .query_all(stmt(
            "SELECT order_id, sub_type, side, platinum, quantity, user_id, first_seen FROM last_seen_orders WHERE item_id = ?",
            vec![input.item_id.into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            Ok(SeenOrder {
                order_id: r.try_get("", "order_id").map_err(|e| db_err(C, e))?,
                sub_type: r.try_get("", "sub_type").map_err(|e| db_err(C, e))?,
                side: r.try_get("", "side").map_err(|e| db_err(C, e))?,
                platinum: r.try_get("", "platinum").map_err(|e| db_err(C, e))?,
                quantity: r.try_get("", "quantity").map_err(|e| db_err(C, e))?,
                user_id: r.try_get("", "user_id").map_err(|e| db_err(C, e))?,
                first_seen: r.try_get("", "first_seen").map_err(|e| db_err(C, e))?,
            })
        })
        .collect::<Result<_, Error>>()?;
    let current = to_seen(input.orders, &swept_at);
    let diff = diff_orders(&previous, &current);

    let vanish_status = if gap { "gap" } else { "pending" };
    for order in &diff.vanished {
        insert_vanish(&txn, input.item_id, order, order.quantity, &swept_at, gap_seconds, "full", vanish_status).await?;
        exec(&txn, C, "DELETE FROM last_seen_orders WHERE order_id = ?", vec![order.order_id.clone().into()]).await?;
    }
    let mut partials = 0;
    for change in &diff.changed {
        let dropped = change.quantity_drop();
        if dropped > 0 {
            insert_vanish(&txn, input.item_id, &change.before, dropped, &swept_at, gap_seconds, "partial", "trade").await?;
            partials += 1;
        }
        exec(
            &txn,
            C,
            "UPDATE last_seen_orders SET platinum = ?, quantity = ?, updated_at = ? WHERE order_id = ?",
            vec![change.after.platinum.into(), change.after.quantity.into(), swept_at.clone().into(), change.after.order_id.clone().into()],
        )
        .await?;
    }
    for order in &diff.new {
        exec(
            &txn,
            C,
            "INSERT OR REPLACE INTO last_seen_orders
                (order_id, item_id, sub_type, side, platinum, quantity, user_id, first_seen, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            vec![
                order.order_id.clone().into(),
                input.item_id.into(),
                order.sub_type.clone().into(),
                order.side.clone().into(),
                order.platinum.into(),
                order.quantity.into(),
                order.user_id.clone().into(),
                order.first_seen.clone().into(),
                swept_at.clone().into(),
            ],
        )
        .await?;
    }

    let summaries = summarize(input.orders);
    for s in &summaries {
        exec(
            &txn,
            C,
            "INSERT INTO sweep_summary
                (item_id, sub_type, swept_at, lane, min_sell, max_buy, sell_count, buy_count, sell_ingame, buy_ingame, top_sells, top_buys)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            vec![
                input.item_id.into(),
                s.sub_type.clone().into(),
                swept_at.clone().into(),
                input.lane.into(),
                s.min_sell.into(),
                s.max_buy.into(),
                s.sell_count.into(),
                s.buy_count.into(),
                s.sell_ingame.into(),
                s.buy_ingame.into(),
                serde_json::to_string(&s.top_sells).unwrap_or_else(|_| "[]".into()).into(),
                serde_json::to_string(&s.top_buys).unwrap_or_else(|_| "[]".into()).into(),
            ],
        )
        .await?;
    }

    exec(
        &txn,
        C,
        "UPDATE sweep_state
         SET last_swept_at = ?, last_attempt_at = ?, first_swept_at = COALESCE(first_swept_at, ?),
             expected_interval_s = ?, consecutive_errors = 0
         WHERE item_id = ?",
        vec![
            swept_at.clone().into(),
            swept_at.clone().into(),
            swept_at.clone().into(),
            input.expected_interval_s.into(),
            input.item_id.into(),
        ],
    )
    .await?;
    txn.commit().await.map_err(|e| db_err(C, e))?;

    Ok(SweepOutcome {
        orders: current.len(),
        groups: summaries.len(),
        new: diff.new.len(),
        vanished: diff.vanished.len(),
        partials,
        gap,
    })
}

/// Active and inactive item counts, and items behind schedule: hot items not swept within
/// `hot_interval_s + 60 s`, or any active item not swept within `2 × cold_expected_s`.
pub async fn item_counts(
    conn: &DatabaseConnection,
    now: DateTime<Utc>,
    hot: &HashSet<String>,
    hot_interval_s: i64,
    cold_expected_s: i64,
) -> Result<ItemCounts, Error> {
    const C: &str = "Collector:ItemCounts";
    let active = active_count(conn).await?;
    let inactive = count(conn, C, "SELECT COUNT(*) AS n FROM sweep_state WHERE active = 0", vec![]).await?;
    let cold_cutoff = ts(now - Duration::seconds(cold_expected_s.saturating_mul(2)));
    let hot_cutoff = ts(now - Duration::seconds(hot_interval_s + 60));
    let mut values: Vec<Value> = vec![cold_cutoff.into()];
    let hot_clause = if hot.is_empty() {
        "0".to_string()
    } else {
        values.extend(hot.iter().map(|id| Value::from(id.clone())));
        values.push(hot_cutoff.into());
        format!("(item_id IN ({}) AND last_swept_at < ?)", placeholders(hot.len()))
    };
    let sql = format!(
        "SELECT COUNT(*) AS n FROM sweep_state
         WHERE active = 1 AND (last_swept_at IS NULL OR last_swept_at < ? OR {hot_clause})"
    );
    let behind = count(conn, C, &sql, values).await?;
    Ok(ItemCounts { active, inactive, behind })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::collector::orders::{V2Order, V2OrderUser};
    use entity::{stock_item, wish_list};
    use service::{StockItemMutation, WishListMutation};

    pub(crate) fn order(id: &str, side: &str, platinum: i64, quantity: i64, rank: i64, user: &str) -> V2Order {
        V2Order {
            id: id.into(),
            side: side.into(),
            platinum,
            quantity,
            rank: Some(rank),
            charges: None,
            subtype: None,
            amber_stars: None,
            cyan_stars: None,
            visible: true,
            item_id: "item1".into(),
            user: V2OrderUser { id: user.into(), ingame_name: user.into(), status: "ingame".into() },
        }
    }

    pub(crate) fn at(minutes: i64) -> DateTime<Utc> {
        parse_ts("2026-09-15T00:00:00Z").unwrap() + chrono::Duration::minutes(minutes)
    }

    pub(crate) async fn setup() -> (tempfile::TempDir, DatabaseConnection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::connect(dir.path()).await.unwrap();
        sync_items(&conn, &[("item1".into(), "slug1".into()), ("item2".into(), "slug2".into())])
            .await
            .unwrap();
        (dir, conn)
    }

    pub(crate) async fn sweep(conn: &DatabaseConnection, item_id: &str, orders: &[V2Order], minutes: i64, expected: i64) -> SweepOutcome {
        apply_sweep(conn, SweepInput {
            item_id,
            lane: "hot",
            orders,
            swept_at: at(minutes),
            expected_interval_s: expected,
            gap_factor: 3.0,
        })
        .await
        .unwrap()
    }

    async fn scalar(conn: &DatabaseConnection, sql: &str) -> i64 {
        conn.query_one(stmt(sql, vec![])).await.unwrap().unwrap().try_get("", "n").unwrap()
    }

    async fn vanished(conn: &DatabaseConnection) -> Vec<(String, String, i64, String, Option<i64>)> {
        conn.query_all(stmt(
            "SELECT order_id, kind, quantity, status, gap_seconds FROM vanished_orders ORDER BY id",
            vec![],
        ))
        .await
        .unwrap()
        .iter()
        .map(|r| {
            (
                r.try_get("", "order_id").unwrap(),
                r.try_get("", "kind").unwrap(),
                r.try_get("", "quantity").unwrap(),
                r.try_get("", "status").unwrap(),
                r.try_get("", "gap_seconds").unwrap(),
            )
        })
        .collect()
    }

    #[tokio::test]
    async fn sync_items_activates_listed_and_deactivates_unlisted() {
        let (_dir, conn) = setup().await;
        sync_items(&conn, &[("item2".into(), "slug2b".into()), ("item3".into(), "slug3".into())])
            .await
            .unwrap();
        assert_eq!(scalar(&conn, "SELECT COUNT(*) AS n FROM sweep_state").await, 3);
        assert_eq!(scalar(&conn, "SELECT active AS n FROM sweep_state WHERE item_id = 'item1'").await, 0);
        assert_eq!(scalar(&conn, "SELECT active AS n FROM sweep_state WHERE item_id = 'item3'").await, 1);
        assert_eq!(active_count(&conn).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn first_sweep_stores_live_set_and_summaries_without_vanishes() {
        let (_dir, conn) = setup().await;
        let orders = vec![order("s1", "sell", 20, 1, 0, "u1"), order("s2", "sell", 90, 1, 5, "u2")];
        let outcome = sweep(&conn, "item1", &orders, 0, 300).await;
        assert_eq!(outcome, SweepOutcome { orders: 2, groups: 2, new: 2, vanished: 0, partials: 0, gap: false });
        assert_eq!(scalar(&conn, "SELECT COUNT(*) AS n FROM last_seen_orders").await, 2);
        assert_eq!(scalar(&conn, "SELECT COUNT(*) AS n FROM sweep_summary").await, 2);
        assert!(vanished(&conn).await.is_empty());
        let row = conn
            .query_one(stmt("SELECT first_swept_at, last_swept_at, expected_interval_s FROM sweep_state WHERE item_id = 'item1'", vec![]))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<Option<String>>("", "first_swept_at").unwrap().unwrap(), "2026-09-15T00:00:00Z");
        assert_eq!(row.try_get::<i64>("", "expected_interval_s").unwrap(), 300);
    }

    #[tokio::test]
    async fn vanished_order_is_pending_and_leaves_the_live_set() {
        let (_dir, conn) = setup().await;
        sweep(&conn, "item1", &[order("s1", "sell", 20, 1, 0, "u1"), order("s2", "sell", 22, 1, 0, "u2")], 0, 300).await;
        let outcome = sweep(&conn, "item1", &[order("s2", "sell", 22, 1, 0, "u2")], 5, 300).await;
        assert_eq!(outcome.vanished, 1);
        assert_eq!(vanished(&conn).await, vec![("s1".into(), "full".into(), 1, "pending".into(), Some(300))]);
        assert_eq!(scalar(&conn, "SELECT COUNT(*) AS n FROM last_seen_orders").await, 1);
    }

    #[tokio::test]
    async fn quantity_drop_is_an_immediate_partial_trade() {
        let (_dir, conn) = setup().await;
        sweep(&conn, "item1", &[order("s1", "sell", 20, 3, 0, "u1")], 0, 300).await;
        let outcome = sweep(&conn, "item1", &[order("s1", "sell", 20, 1, 0, "u1")], 5, 300).await;
        assert_eq!(outcome.partials, 1);
        assert_eq!(vanished(&conn).await, vec![("s1".into(), "partial".into(), 2, "trade".into(), Some(300))]);
        assert_eq!(scalar(&conn, "SELECT quantity AS n FROM last_seen_orders WHERE order_id = 's1'").await, 1);
    }

    #[tokio::test]
    async fn price_edit_is_not_a_vanish() {
        let (_dir, conn) = setup().await;
        sweep(&conn, "item1", &[order("s1", "sell", 20, 1, 0, "u1")], 0, 300).await;
        sweep(&conn, "item1", &[order("s1", "sell", 18, 1, 0, "u1")], 5, 300).await;
        assert!(vanished(&conn).await.is_empty());
        assert_eq!(scalar(&conn, "SELECT platinum AS n FROM last_seen_orders WHERE order_id = 's1'").await, 18);
    }

    #[tokio::test]
    async fn vanish_after_a_long_gap_is_marked_gap() {
        let (_dir, conn) = setup().await;
        sweep(&conn, "item1", &[order("s1", "sell", 20, 1, 0, "u1")], 0, 300).await;
        let outcome = sweep(&conn, "item1", &[], 16, 300).await;
        assert!(outcome.gap);
        assert_eq!(vanished(&conn).await[0].3, "gap");
    }

    #[tokio::test]
    async fn gap_check_uses_the_larger_expected_interval() {
        let (_dir, conn) = setup().await;
        sweep(&conn, "item1", &[order("s1", "sell", 20, 1, 0, "u1")], 0, 1500).await;
        let outcome = sweep(&conn, "item1", &[], 20, 300).await;
        assert!(!outcome.gap);
        assert_eq!(vanished(&conn).await[0].3, "pending");
    }

    #[tokio::test]
    async fn hot_item_ids_come_from_stock_and_wish_list() {
        let (_dir, conn) = setup().await;
        StockItemMutation::create(
            &conn,
            stock_item::Model::new("item1".into(), "slug1".into(), "One".into(), "".into(), None, 10, 1, false, Default::default()),
        )
        .await
        .unwrap();
        WishListMutation::create(
            &conn,
            &wish_list::Model::new("item2".into(), "slug2".into(), "Two".into(), "".into(), None, 1, Default::default()),
        )
        .await
        .unwrap();
        let ids = hot_item_ids(&conn).await.unwrap();
        assert_eq!(ids, HashSet::from(["item1".to_string(), "item2".to_string()]));
        let targets = hot_targets(&conn, &HashSet::from(["item2".to_string()])).await.unwrap();
        assert_eq!(targets, vec![SweepTarget { item_id: "item2".into(), slug: "slug2".into(), last_attempt_at: None }]);
        assert!(hot_targets(&conn, &HashSet::new()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn cold_candidates_put_never_attempted_first_then_oldest() {
        let (_dir, conn) = setup().await;
        mark_attempt(&conn, "item1", at(0)).await.unwrap();
        let first = cold_candidates(&conn, 10).await.unwrap();
        assert_eq!(first.iter().map(|t| t.item_id.as_str()).collect::<Vec<_>>(), vec!["item2", "item1"]);
        mark_attempt(&conn, "item2", at(1)).await.unwrap();
        let second = cold_candidates(&conn, 10).await.unwrap();
        assert_eq!(second[0].item_id, "item1");
    }

    #[tokio::test]
    async fn errors_and_not_found_update_sweep_state() {
        let (_dir, conn) = setup().await;
        record_error(&conn, "item1").await.unwrap();
        record_error(&conn, "item1").await.unwrap();
        deactivate(&conn, "item2").await.unwrap();
        assert_eq!(scalar(&conn, "SELECT consecutive_errors AS n FROM sweep_state WHERE item_id = 'item1'").await, 2);
        assert_eq!(active_count(&conn).await.unwrap(), 1);
        sweep(&conn, "item1", &[], 0, 300).await;
        assert_eq!(scalar(&conn, "SELECT consecutive_errors AS n FROM sweep_state WHERE item_id = 'item1'").await, 0);
    }

    #[tokio::test]
    async fn item_counts_report_items_behind() {
        let (_dir, conn) = setup().await;
        sweep(&conn, "item1", &[], 0, 300).await;
        let hot = HashSet::from(["item1".to_string()]);
        // item1 is hot and last swept 10 min ago (> 300 s + 60 s); item2 was never swept.
        let counts = item_counts(&conn, at(10), &hot, 300, 3600).await.unwrap();
        assert_eq!(counts, ItemCounts { active: 2, inactive: 0, behind: 2 });
        let counts = item_counts(&conn, at(5), &hot, 300, 3600).await.unwrap();
        assert_eq!(counts.behind, 1);
    }
}
