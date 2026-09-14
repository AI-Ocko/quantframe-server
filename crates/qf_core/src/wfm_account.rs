use chrono::{DateTime, Utc};
use service::sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};
use utils::{get_location, Error};

use crate::crypto::{jwt_expiry, SecretKey};

#[derive(Debug, Clone)]
pub struct StoredAccount {
    pub token: String,
    pub token_expires_at: Option<DateTime<Utc>>,
    pub wfm_user_id: String,
    pub username: String,
}

fn db_err(component: &str, e: impl std::fmt::Display) -> Error {
    Error::new(component, e.to_string(), get_location!())
}

pub async fn save(
    conn: &DatabaseConnection,
    key: &SecretKey,
    token: &str,
    wfm_user_id: &str,
    username: &str,
) -> Result<(), Error> {
    let (ciphertext, nonce) = key.encrypt(token.as_bytes())?;
    let expires = jwt_expiry(token).map(|d| d.to_rfc3339());
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR REPLACE INTO wfm_account
            (id, token_ciphertext, nonce, token_expires_at, wfm_user_id, username, created_at)
         VALUES (1, ?, ?, ?, ?, ?, ?)",
        [
            Value::Bytes(Some(Box::new(ciphertext))),
            Value::Bytes(Some(Box::new(nonce))),
            expires.into(),
            wfm_user_id.to_string().into(),
            username.to_string().into(),
            Utc::now().to_rfc3339().into(),
        ],
    ))
    .await
    .map_err(|e| db_err("WfmAccount:Save", e))?;
    Ok(())
}

pub async fn load(conn: &DatabaseConnection, key: &SecretKey) -> Result<Option<StoredAccount>, Error> {
    let row = conn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT token_ciphertext, nonce, token_expires_at, wfm_user_id, username FROM wfm_account WHERE id = 1"
                .to_string(),
        ))
        .await
        .map_err(|e| db_err("WfmAccount:Load", e))?;
    let Some(row) = row else { return Ok(None) };
    let ciphertext: Vec<u8> = row.try_get("", "token_ciphertext").map_err(|e| db_err("WfmAccount:Load", e))?;
    let nonce: Vec<u8> = row.try_get("", "nonce").map_err(|e| db_err("WfmAccount:Load", e))?;
    let expires: Option<String> = row.try_get("", "token_expires_at").map_err(|e| db_err("WfmAccount:Load", e))?;
    let token = String::from_utf8(key.decrypt(&ciphertext, &nonce)?).map_err(|e| db_err("WfmAccount:Load", e))?;
    Ok(Some(StoredAccount {
        token,
        token_expires_at: expires
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|d| d.with_timezone(&Utc)),
        wfm_user_id: row.try_get("", "wfm_user_id").map_err(|e| db_err("WfmAccount:Load", e))?,
        username: row.try_get("", "username").map_err(|e| db_err("WfmAccount:Load", e))?,
    }))
}

pub async fn delete(conn: &DatabaseConnection) -> Result<(), Error> {
    conn.execute(Statement::from_string(DbBackend::Sqlite, "DELETE FROM wfm_account".to_string()))
        .await
        .map_err(|e| db_err("WfmAccount:Delete", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::SecretKey;

    fn key(byte: &str) -> SecretKey {
        SecretKey::from_hex(&byte.repeat(32)).unwrap()
    }

    #[tokio::test]
    async fn save_load_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::connect(dir.path()).await.unwrap();
        let k = key("11");
        assert!(load(&conn, &k).await.unwrap().is_none());

        save(&conn, &k, "JWT header.eyJleHAiOjE3OTQ2MTQ0MDB9.sig", "u1", "Tenno").await.unwrap();
        let account = load(&conn, &k).await.unwrap().unwrap();
        assert_eq!(account.token, "JWT header.eyJleHAiOjE3OTQ2MTQ0MDB9.sig");
        assert_eq!(account.username, "Tenno");
        assert_eq!(account.wfm_user_id, "u1");
        assert_eq!(account.token_expires_at.unwrap().timestamp(), 1794614400);

        assert!(load(&conn, &key("22")).await.is_err(), "wrong key must not decrypt");

        delete(&conn).await.unwrap();
        assert!(load(&conn, &k).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn token_is_not_stored_in_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::connect(dir.path()).await.unwrap();
        save(&conn, &key("11"), "super-secret-token", "u1", "Tenno").await.unwrap();
        conn.close().await.unwrap();
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                let raw = std::fs::read(&path).unwrap();
                let needle = b"super-secret-token";
                assert!(!raw.windows(needle.len()).any(|w| w == needle), "{} contains the token", path.display());
            }
        }
    }
}
