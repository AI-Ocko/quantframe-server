use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::Error;

use crate::collector::store::{count, exec};
use crate::collector::{db_err, stmt, ts};

use super::DRY_RUN_RETENTION_DAYS;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraderOptions {
    pub dry_run: bool,
    pub delete_buy_orders_on_stop: bool,
    pub last_stop_reason: Option<String>,
    pub last_stop_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DryRunEntry {
    pub id: i64,
    pub at: String,
    pub action: String,
    pub side: String,
    pub item_id: String,
    pub sub_type: String,
    pub price: Option<i64>,
    pub quantity: Option<i64>,
    pub reason: String,
    pub forced_by: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DryRunPage {
    pub total: i64,
    pub page: i64,
    pub limit: i64,
    pub results: Vec<DryRunEntry>,
}

pub async fn load_options(conn: &DatabaseConnection) -> Result<TraderOptions, Error> {
    const C: &str = "Trader:LoadOptions";
    let row = conn
        .query_one(stmt(
            "SELECT dry_run, delete_buy_orders_on_stop, last_stop_reason, last_stop_at
             FROM trader_state WHERE id = 1",
            vec![],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .ok_or_else(|| db_err(C, "trader_state row is missing"))?;
    Ok(TraderOptions {
        dry_run: row.try_get::<i64>("", "dry_run").map_err(|e| db_err(C, e))? != 0,
        delete_buy_orders_on_stop: row.try_get::<i64>("", "delete_buy_orders_on_stop").map_err(|e| db_err(C, e))? != 0,
        last_stop_reason: row.try_get("", "last_stop_reason").map_err(|e| db_err(C, e))?,
        last_stop_at: row.try_get("", "last_stop_at").map_err(|e| db_err(C, e))?,
    })
}

pub async fn save_flags(conn: &DatabaseConnection, dry_run: bool, delete_buy_orders_on_stop: bool) -> Result<(), Error> {
    exec(
        conn,
        "Trader:SaveFlags",
        "UPDATE trader_state SET dry_run = ?, delete_buy_orders_on_stop = ? WHERE id = 1",
        vec![(dry_run as i64).into(), (delete_buy_orders_on_stop as i64).into()],
    )
    .await
    .map(|_| ())
}

pub async fn record_stop(conn: &DatabaseConnection, reason: &str, at: DateTime<Utc>) -> Result<(), Error> {
    exec(
        conn,
        "Trader:RecordStop",
        "UPDATE trader_state SET last_stop_reason = ?, last_stop_at = ? WHERE id = 1",
        vec![reason.into(), ts(at).into()],
    )
    .await
    .map(|_| ())
}

pub async fn insert_dry_run(conn: &DatabaseConnection, entry: &DryRunEntry) -> Result<(), Error> {
    exec(
        conn,
        "Trader:InsertDryRun",
        "INSERT INTO dry_run_log (at, action, side, item_id, sub_type, price, quantity, reason, forced_by)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        vec![
            entry.at.clone().into(),
            entry.action.clone().into(),
            entry.side.clone().into(),
            entry.item_id.clone().into(),
            entry.sub_type.clone().into(),
            entry.price.into(),
            entry.quantity.into(),
            entry.reason.clone().into(),
            entry.forced_by.clone().into(),
        ],
    )
    .await
    .map(|_| ())
}

/// Newest first. `page` starts at 1; `limit` is clamped to 1..=500.
pub async fn dry_run_page(conn: &DatabaseConnection, page: i64, limit: i64) -> Result<DryRunPage, Error> {
    const C: &str = "Trader:DryRunPage";
    let page = page.max(1);
    let limit = limit.clamp(1, 500);
    let total = count(conn, C, "SELECT COUNT(*) AS n FROM dry_run_log", vec![]).await?;
    let results = conn
        .query_all(stmt(
            "SELECT id, at, action, side, item_id, sub_type, price, quantity, reason, forced_by
             FROM dry_run_log ORDER BY id DESC LIMIT ? OFFSET ?",
            vec![limit.into(), ((page - 1) * limit).into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            Ok(DryRunEntry {
                id: r.try_get("", "id").map_err(|e| db_err(C, e))?,
                at: r.try_get("", "at").map_err(|e| db_err(C, e))?,
                action: r.try_get("", "action").map_err(|e| db_err(C, e))?,
                side: r.try_get("", "side").map_err(|e| db_err(C, e))?,
                item_id: r.try_get("", "item_id").map_err(|e| db_err(C, e))?,
                sub_type: r.try_get("", "sub_type").map_err(|e| db_err(C, e))?,
                price: r.try_get("", "price").map_err(|e| db_err(C, e))?,
                quantity: r.try_get("", "quantity").map_err(|e| db_err(C, e))?,
                reason: r.try_get("", "reason").map_err(|e| db_err(C, e))?,
                forced_by: r.try_get("", "forced_by").map_err(|e| db_err(C, e))?,
            })
        })
        .collect::<Result<_, Error>>()?;
    Ok(DryRunPage { total, page, limit, results })
}

pub async fn prune_dry_run(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<u64, Error> {
    exec(
        conn,
        "Trader:PruneDryRun",
        "DELETE FROM dry_run_log WHERE at < ?",
        vec![ts(now - Duration::days(DRY_RUN_RETENTION_DAYS)).into()],
    )
    .await
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::collector::parse_ts;

    pub(crate) async fn db() -> (tempfile::TempDir, DatabaseConnection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::connect(dir.path()).await.unwrap();
        (dir, conn)
    }

    fn entry(at: &str, action: &str) -> DryRunEntry {
        DryRunEntry {
            id: 0,
            at: at.into(),
            action: action.into(),
            side: "buy".into(),
            item_id: "item1".into(),
            sub_type: "rank=0".into(),
            price: Some(17),
            quantity: Some(1),
            reason: "Create".into(),
            forced_by: "global".into(),
        }
    }

    #[tokio::test]
    async fn options_default_to_dry_run_and_persist() {
        let (_dir, conn) = db().await;
        let options = load_options(&conn).await.unwrap();
        assert_eq!(
            options,
            TraderOptions { dry_run: true, delete_buy_orders_on_stop: false, last_stop_reason: None, last_stop_at: None }
        );
        save_flags(&conn, false, true).await.unwrap();
        record_stop(&conn, "Stop button", parse_ts("2026-09-16T00:00:00Z").unwrap()).await.unwrap();
        let options = load_options(&conn).await.unwrap();
        assert!(!options.dry_run && options.delete_buy_orders_on_stop);
        assert_eq!(options.last_stop_reason.as_deref(), Some("Stop button"));
        assert_eq!(options.last_stop_at.as_deref(), Some("2026-09-16T00:00:00Z"));
    }

    #[tokio::test]
    async fn dry_run_log_pages_newest_first_and_prunes_old_rows() {
        let (_dir, conn) = db().await;
        insert_dry_run(&conn, &entry("2026-08-01T00:00:00Z", "create")).await.unwrap();
        insert_dry_run(&conn, &entry("2026-09-15T00:00:00Z", "update")).await.unwrap();
        insert_dry_run(&conn, &entry("2026-09-15T00:01:00Z", "delete")).await.unwrap();
        let page = dry_run_page(&conn, 1, 2).await.unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.results.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(), vec!["delete", "update"]);
        assert_eq!(dry_run_page(&conn, 2, 2).await.unwrap().results[0].action, "create");
        let removed = prune_dry_run(&conn, parse_ts("2026-09-16T00:00:00Z").unwrap()).await.unwrap();
        assert_eq!(removed, 1);
        assert_eq!(dry_run_page(&conn, 1, 10).await.unwrap().total, 2);
    }
}
