use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use service::sea_orm::{ConnectionTrait, DatabaseConnection, QueryResult, Value};
use utils::Error;

use super::{RawTrade, Resolution};
use crate::collector::store::{count, exec};
use crate::collector::{db_err, stmt, ts};

pub const RETENTION_DAYS: i64 = 90;
pub const APPLIED: &str = "applied";
pub const NEEDS_REVIEW: &str = "needs_review";
pub const IGNORED: &str = "ignored";

const COLUMNS: &str = "event_id, device_name, received_at, detected_at, status, reason, payload, resolution, reviewed_at";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HelperEvent {
    pub event_id: String,
    pub device_name: String,
    pub received_at: String,
    pub detected_at: String,
    pub status: String,
    pub reason: Option<String>,
    pub payload: RawTrade,
    pub resolution: Option<Resolution>,
    pub reviewed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventPage {
    pub total: i64,
    pub page: i64,
    pub limit: i64,
    pub results: Vec<HelperEvent>,
}

fn from_row(c: &str, row: &QueryResult) -> Result<HelperEvent, Error> {
    let payload: String = row.try_get("", "payload").map_err(|e| db_err(c, e))?;
    let resolution: Option<String> = row.try_get("", "resolution").map_err(|e| db_err(c, e))?;
    Ok(HelperEvent {
        event_id: row.try_get("", "event_id").map_err(|e| db_err(c, e))?,
        device_name: row.try_get("", "device_name").map_err(|e| db_err(c, e))?,
        received_at: row.try_get("", "received_at").map_err(|e| db_err(c, e))?,
        detected_at: row.try_get("", "detected_at").map_err(|e| db_err(c, e))?,
        status: row.try_get("", "status").map_err(|e| db_err(c, e))?,
        reason: row.try_get("", "reason").map_err(|e| db_err(c, e))?,
        payload: serde_json::from_str(&payload).map_err(|e| db_err(c, e))?,
        resolution: resolution.as_deref().map(serde_json::from_str).transpose().map_err(|e| db_err(c, e))?,
        reviewed_at: row.try_get("", "reviewed_at").map_err(|e| db_err(c, e))?,
    })
}

pub async fn exists(conn: &DatabaseConnection, event_id: &str) -> Result<bool, Error> {
    Ok(count(conn, "HelperEvents:Exists", "SELECT COUNT(*) AS n FROM helper_events WHERE event_id = ?", vec![event_id.into()]).await? > 0)
}

pub async fn insert(conn: &DatabaseConnection, event: &HelperEvent) -> Result<(), Error> {
    const C: &str = "HelperEvents:Insert";
    let payload = serde_json::to_string(&event.payload).map_err(|e| db_err(C, e))?;
    let resolution = event.resolution.as_ref().map(serde_json::to_string).transpose().map_err(|e| db_err(C, e))?;
    exec(
        conn,
        C,
        &format!("INSERT INTO helper_events ({COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"),
        vec![
            event.event_id.clone().into(),
            event.device_name.clone().into(),
            event.received_at.clone().into(),
            event.detected_at.clone().into(),
            event.status.clone().into(),
            event.reason.clone().into(),
            payload.into(),
            resolution.into(),
            event.reviewed_at.clone().into(),
        ],
    )
    .await?;
    Ok(())
}

pub async fn get(conn: &DatabaseConnection, event_id: &str) -> Result<Option<HelperEvent>, Error> {
    const C: &str = "HelperEvents:Get";
    conn.query_one(stmt(&format!("SELECT {COLUMNS} FROM helper_events WHERE event_id = ?"), vec![event_id.into()]))
        .await
        .map_err(|e| db_err(C, e))?
        .map(|row| from_row(C, &row))
        .transpose()
}

/// Newest first. `status = None` lists everything. `limit` is clamped to 1..=200.
pub async fn list(conn: &DatabaseConnection, status: Option<&str>, page: i64, limit: i64) -> Result<EventPage, Error> {
    const C: &str = "HelperEvents:List";
    let page = page.max(1);
    let limit = limit.clamp(1, 200);
    let (filter, values): (&str, Vec<Value>) = match status {
        Some(status) => ("WHERE status = ?", vec![status.into()]),
        None => ("", vec![]),
    };
    let total = count(conn, C, &format!("SELECT COUNT(*) AS n FROM helper_events {filter}"), values.clone()).await?;
    let mut values = values;
    values.push(limit.into());
    values.push(((page - 1) * limit).into());
    let results = conn
        .query_all(stmt(
            &format!("SELECT {COLUMNS} FROM helper_events {filter} ORDER BY received_at DESC, rowid DESC LIMIT ? OFFSET ?"),
            values,
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|row| from_row(C, row))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EventPage { total, page, limit, results })
}

/// Returns false when no row has that id.
pub async fn set_status(
    conn: &DatabaseConnection,
    event_id: &str,
    status: &str,
    reason: Option<&str>,
    resolution: Option<&Resolution>,
    reviewed_at: Option<DateTime<Utc>>,
) -> Result<bool, Error> {
    const C: &str = "HelperEvents:SetStatus";
    let resolution = resolution.map(serde_json::to_string).transpose().map_err(|e| db_err(C, e))?;
    let changed = exec(
        conn,
        C,
        "UPDATE helper_events SET status = ?, reason = ?, resolution = ?, reviewed_at = ? WHERE event_id = ?",
        vec![
            status.into(),
            reason.map(str::to_string).into(),
            resolution.into(),
            reviewed_at.map(ts).into(),
            event_id.into(),
        ],
    )
    .await?;
    Ok(changed > 0)
}

/// Deletes events received more than 90 days ago (amendment E5).
pub async fn apply_retention(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<u64, Error> {
    exec(
        conn,
        "HelperEvents:Retention",
        "DELETE FROM helper_events WHERE received_at < ?",
        vec![ts(now - Duration::days(RETENTION_DAYS)).into()],
    )
    .await
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::collector::parse_ts;
    use crate::helper_link::trades::{Direction, RawItem, ResolvedItem};
    use crate::trader::store::tests::db;

    pub(crate) fn at(text: &str) -> DateTime<Utc> {
        parse_ts(text).unwrap()
    }

    pub(crate) fn sale_trade() -> RawTrade {
        RawTrade {
            player_name: "PlayerB".into(),
            ee_timestamp: "1170.388".into(),
            offered: vec![RawItem { name: "Arcane Nullifier".into(), quantity: 1, rank: Some(5) }],
            received: vec![RawItem { name: "Platinum".into(), quantity: 70, rank: None }],
        }
    }

    pub(crate) fn event(id: &str, received_at: &str, status: &str) -> HelperEvent {
        HelperEvent {
            event_id: id.into(),
            device_name: "gaming-pc".into(),
            received_at: received_at.into(),
            detected_at: received_at.into(),
            status: status.into(),
            reason: None,
            payload: sale_trade(),
            resolution: None,
            reviewed_at: None,
        }
    }

    fn resolution() -> Resolution {
        Resolution {
            direction: Some(Direction::Sale),
            platinum: 70,
            items: vec![ResolvedItem {
                name: "Arcane Nullifier".into(),
                slug: "arcane_nullifier".into(),
                wfm_id: "id_nullifier".into(),
                item_name: "Arcane Nullifier".into(),
                sub_type: Some(utils::SubType::rank(5)),
                quantity: 1,
                price: 70,
                matched_by: "name".into(),
            }],
            extras: vec![],
        }
    }

    #[tokio::test]
    async fn insert_get_and_exists_round_trip_payload_and_resolution() {
        let (_dir, conn) = db().await;
        let mut stored = event("a".repeat(64).as_str(), "2026-09-15T10:00:00Z", APPLIED);
        stored.resolution = Some(resolution());
        assert!(!exists(&conn, &stored.event_id).await.unwrap());
        insert(&conn, &stored).await.unwrap();
        assert!(exists(&conn, &stored.event_id).await.unwrap());
        assert_eq!(get(&conn, &stored.event_id).await.unwrap(), Some(stored.clone()));
        assert!(insert(&conn, &stored).await.is_err(), "the primary key rejects a duplicate");
        assert_eq!(get(&conn, "missing").await.unwrap(), None);
    }

    #[tokio::test]
    async fn list_filters_by_status_and_pages_newest_first() {
        let (_dir, conn) = db().await;
        insert(&conn, &event("e1", "2026-09-15T10:00:00Z", APPLIED)).await.unwrap();
        insert(&conn, &event("e2", "2026-09-15T10:01:00Z", NEEDS_REVIEW)).await.unwrap();
        insert(&conn, &event("e3", "2026-09-15T10:02:00Z", NEEDS_REVIEW)).await.unwrap();
        let all = list(&conn, None, 1, 50).await.unwrap();
        assert_eq!(all.total, 3);
        assert_eq!(all.results.iter().map(|e| e.event_id.as_str()).collect::<Vec<_>>(), ["e3", "e2", "e1"]);
        let review = list(&conn, Some(NEEDS_REVIEW), 1, 1).await.unwrap();
        assert_eq!((review.total, review.results.len(), review.results[0].event_id.as_str()), (2, 1, "e3"));
        let second = list(&conn, Some(NEEDS_REVIEW), 2, 1).await.unwrap();
        assert_eq!(second.results[0].event_id, "e2");
        assert_eq!(list(&conn, Some(NEEDS_REVIEW), 0, 0).await.unwrap().limit, 1, "page and limit are clamped");
    }

    #[tokio::test]
    async fn set_status_updates_reason_resolution_and_reviewed_at() {
        let (_dir, conn) = db().await;
        insert(&conn, &event("e1", "2026-09-15T10:00:00Z", NEEDS_REVIEW)).await.unwrap();
        assert!(set_status(&conn, "e1", APPLIED, None, Some(&resolution()), Some(at("2026-09-15T11:00:00Z"))).await.unwrap());
        let updated = get(&conn, "e1").await.unwrap().unwrap();
        assert_eq!(updated.status, APPLIED);
        assert_eq!(updated.resolution, Some(resolution()));
        assert_eq!(updated.reviewed_at.as_deref(), Some("2026-09-15T11:00:00Z"));
        assert!(set_status(&conn, "e1", IGNORED, Some("reviewed"), None, None).await.unwrap());
        assert_eq!(get(&conn, "e1").await.unwrap().unwrap().reason.as_deref(), Some("reviewed"));
        assert!(!set_status(&conn, "nope", IGNORED, None, None, None).await.unwrap());
    }

    #[tokio::test]
    async fn retention_deletes_events_older_than_90_days() {
        let (_dir, conn) = db().await;
        insert(&conn, &event("old", "2026-06-01T10:00:00Z", APPLIED)).await.unwrap();
        insert(&conn, &event("new", "2026-09-15T10:00:00Z", APPLIED)).await.unwrap();
        assert_eq!(apply_retention(&conn, at("2026-09-15T12:00:00Z")).await.unwrap(), 1);
        assert_eq!(list(&conn, None, 1, 10).await.unwrap().results[0].event_id, "new");
    }
}
