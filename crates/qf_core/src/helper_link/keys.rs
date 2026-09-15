use chrono::{DateTime, Utc};
use serde::Serialize;
use service::sea_orm::{ConnectionTrait, DatabaseConnection, QueryResult};
use sha2::{Digest, Sha256};
use utils::{get_location, Error};

use crate::collector::store::exec;
use crate::collector::{db_err, stmt, ts};

pub const KEY_PREFIX: &str = "qfh_";
const MAX_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HelperDevice {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub last_seen_at: Option<String>,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CreatedDevice {
    pub device: HelperDevice,
    /// Shown once; only its SHA-256 is stored (amendment D3).
    pub key: String,
}

/// A device whose key is valid and not revoked.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceIdentity {
    pub id: i64,
    pub name: String,
}

pub fn hash_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

pub fn generate_key() -> Result<String, Error> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| {
        Error::new("HelperLink:Key", format!("OS random number generator unavailable: {}", e), get_location!())
    })?;
    Ok(format!("{}{}", KEY_PREFIX, hex::encode(bytes)))
}

fn device_from_row(component: &str, row: &QueryResult) -> Result<HelperDevice, Error> {
    Ok(HelperDevice {
        id: row.try_get("", "id").map_err(|e| db_err(component, e))?,
        name: row.try_get("", "name").map_err(|e| db_err(component, e))?,
        created_at: row.try_get("", "created_at").map_err(|e| db_err(component, e))?,
        last_seen_at: row.try_get("", "last_seen_at").map_err(|e| db_err(component, e))?,
        revoked_at: row.try_get("", "revoked_at").map_err(|e| db_err(component, e))?,
    })
}

pub async fn create(conn: &DatabaseConnection, name: &str, now: DateTime<Utc>) -> Result<CreatedDevice, Error> {
    const C: &str = "HelperLink:Create";
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(Error::new(C, "Device name must be 1-64 characters", get_location!()));
    }
    let key = generate_key()?;
    let key_hash = hash_key(&key);
    exec(
        conn,
        C,
        "INSERT INTO helper_keys (name, key_hash, created_at) VALUES (?, ?, ?)",
        vec![name.into(), key_hash.clone().into(), ts(now).into()],
    )
    .await?;
    let row = conn
        .query_one(stmt(
            "SELECT id, name, created_at, last_seen_at, revoked_at FROM helper_keys WHERE key_hash = ?",
            vec![key_hash.into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .ok_or_else(|| db_err(C, "created device row is missing"))?;
    Ok(CreatedDevice { device: device_from_row(C, &row)?, key })
}

/// Active devices first, newest first within each group.
pub async fn list(conn: &DatabaseConnection) -> Result<Vec<HelperDevice>, Error> {
    const C: &str = "HelperLink:List";
    conn.query_all(stmt(
        "SELECT id, name, created_at, last_seen_at, revoked_at FROM helper_keys
         ORDER BY revoked_at IS NOT NULL, id DESC",
        vec![],
    ))
    .await
    .map_err(|e| db_err(C, e))?
    .iter()
    .map(|row| device_from_row(C, row))
    .collect()
}

/// Returns false when the device doesn't exist or was already revoked.
pub async fn revoke(conn: &DatabaseConnection, id: i64, now: DateTime<Utc>) -> Result<bool, Error> {
    let changed = exec(
        conn,
        "HelperLink:Revoke",
        "UPDATE helper_keys SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL",
        vec![ts(now).into(), id.into()],
    )
    .await?;
    Ok(changed > 0)
}

/// The device for a valid, non-revoked key; updates `last_seen_at`.
pub async fn authenticate(
    conn: &DatabaseConnection,
    key: &str,
    now: DateTime<Utc>,
) -> Result<Option<DeviceIdentity>, Error> {
    const C: &str = "HelperLink:Authenticate";
    if !key.starts_with(KEY_PREFIX) {
        return Ok(None);
    }
    let Some(row) = conn
        .query_one(stmt(
            "SELECT id, name FROM helper_keys WHERE key_hash = ? AND revoked_at IS NULL",
            vec![hash_key(key).into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
    else {
        return Ok(None);
    };
    let identity = DeviceIdentity {
        id: row.try_get("", "id").map_err(|e| db_err(C, e))?,
        name: row.try_get("", "name").map_err(|e| db_err(C, e))?,
    };
    exec(
        conn,
        C,
        "UPDATE helper_keys SET last_seen_at = ? WHERE id = ?",
        vec![ts(now).into(), identity.id.into()],
    )
    .await?;
    Ok(Some(identity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;
    use crate::trader::store::tests::db;

    fn at(text: &str) -> DateTime<Utc> {
        parse_ts(text).unwrap()
    }

    #[tokio::test]
    async fn create_authenticate_and_revoke() {
        let (_dir, conn) = db().await;
        let created = create(&conn, "  gaming-pc  ", at("2026-09-17T10:00:00Z")).await.unwrap();
        assert!(created.key.starts_with(KEY_PREFIX));
        assert_eq!(created.key.len(), KEY_PREFIX.len() + 64);
        assert_eq!(created.device.name, "gaming-pc");
        assert_eq!(created.device.last_seen_at, None);

        let identity = authenticate(&conn, &created.key, at("2026-09-17T10:05:00Z")).await.unwrap();
        assert_eq!(identity, Some(DeviceIdentity { id: created.device.id, name: "gaming-pc".into() }));
        assert_eq!(list(&conn).await.unwrap()[0].last_seen_at.as_deref(), Some("2026-09-17T10:05:00Z"));

        assert_eq!(authenticate(&conn, "qfh_wrong", at("2026-09-17T10:06:00Z")).await.unwrap(), None);
        assert_eq!(authenticate(&conn, "not-a-key", at("2026-09-17T10:06:00Z")).await.unwrap(), None);

        assert!(revoke(&conn, created.device.id, at("2026-09-17T11:00:00Z")).await.unwrap());
        assert!(!revoke(&conn, created.device.id, at("2026-09-17T11:01:00Z")).await.unwrap(), "already revoked");
        assert_eq!(authenticate(&conn, &created.key, at("2026-09-17T11:02:00Z")).await.unwrap(), None);
        assert_eq!(list(&conn).await.unwrap()[0].revoked_at.as_deref(), Some("2026-09-17T11:00:00Z"));
    }

    #[tokio::test]
    async fn only_the_hash_is_stored_and_active_devices_list_first() {
        let (_dir, conn) = db().await;
        let first = create(&conn, "old-pc", at("2026-09-17T10:00:00Z")).await.unwrap();
        let second = create(&conn, "gaming-pc", at("2026-09-17T10:01:00Z")).await.unwrap();
        revoke(&conn, second.device.id, at("2026-09-17T10:02:00Z")).await.unwrap();

        let stored = conn
            .query_all(stmt("SELECT key_hash FROM helper_keys", vec![]))
            .await
            .unwrap()
            .iter()
            .map(|r| r.try_get::<String>("", "key_hash").unwrap())
            .collect::<Vec<_>>();
        assert!(stored.contains(&hash_key(&first.key)));
        assert!(stored.iter().all(|h| !h.contains(&first.key) && !h.contains(&second.key)));

        let names = list(&conn).await.unwrap().into_iter().map(|d| d.name).collect::<Vec<_>>();
        assert_eq!(names, vec!["old-pc", "gaming-pc"]);
    }

    #[tokio::test]
    async fn device_names_must_be_1_to_64_characters() {
        let (_dir, conn) = db().await;
        let now = at("2026-09-17T10:00:00Z");
        assert!(create(&conn, "   ", now).await.is_err());
        assert!(create(&conn, &"x".repeat(65), now).await.is_err());
        assert!(create(&conn, &"x".repeat(64), now).await.is_ok());
    }
}
