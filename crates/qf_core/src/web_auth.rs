use std::path::Path;

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::Utc;
use service::sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use utils::{get_location, Error};

const MIN_PASSWORD_LEN: usize = 12;

pub fn hash_password(password: &str) -> Result<String, Error> {
    let mut salt_bytes = [0u8; 16];
    getrandom::getrandom(&mut salt_bytes)
        .map_err(|e| Error::new("WebAuth:Hash", format!("{:?}", e), get_location!()))?;
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|e| Error::new("WebAuth:Hash", format!("{:?}", e), get_location!()))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| Error::new("WebAuth:Hash", format!("{:?}", e), get_location!()))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

async fn stored_hash(conn: &DatabaseConnection) -> Result<Option<String>, Error> {
    let row = conn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT password_hash FROM web_auth WHERE id = 1".to_string(),
        ))
        .await
        .map_err(|e| Error::new("WebAuth:Load", e.to_string(), get_location!()))?;
    match row {
        Some(row) => Ok(Some(
            row.try_get("", "password_hash")
                .map_err(|e| Error::new("WebAuth:Load", e.to_string(), get_location!()))?,
        )),
        None => Ok(None),
    }
}

/// The password file is the source of truth. Its password is re-hashed whenever it no longer
/// matches the stored hash. If the file is missing, the stored hash is used.
pub async fn ensure_password(conn: &DatabaseConnection, password_file: &Path) -> Result<String, Error> {
    let existing = stored_hash(conn).await?;
    let from_file = std::fs::read_to_string(password_file).ok().map(|s| s.trim().to_string());

    match (from_file, existing) {
        (Some(password), existing) => {
            if password.chars().count() < MIN_PASSWORD_LEN {
                return Err(Error::new(
                    "WebAuth:Ensure",
                    format!("Web password must be at least {} characters", MIN_PASSWORD_LEN),
                    get_location!(),
                ));
            }
            if let Some(hash) = existing.filter(|h| verify_password(&password, h)) {
                return Ok(hash);
            }
            let hash = hash_password(&password)?;
            conn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR REPLACE INTO web_auth (id, password_hash, updated_at) VALUES (1, ?, ?)",
                [hash.clone().into(), Utc::now().to_rfc3339().into()],
            ))
            .await
            .map_err(|e| Error::new("WebAuth:Ensure", e.to_string(), get_location!()))?;
            Ok(hash)
        }
        (None, Some(hash)) => Ok(hash),
        (None, None) => Err(Error::new(
            "WebAuth:Ensure",
            format!("No web password set: create {}", password_file.display()),
            get_location!(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let hash = hash_password("correct horse battery").unwrap();
        assert!(verify_password("correct horse battery", &hash));
        assert!(!verify_password("wrong", &hash));
        assert!(!verify_password("x", "not-a-hash"));
    }

    #[tokio::test]
    async fn file_is_source_of_truth() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::connect(dir.path()).await.unwrap();
        let file = dir.path().join("pw");

        assert!(ensure_password(&conn, &file).await.is_err(), "no file and no row");

        std::fs::write(&file, "first-password-123\n").unwrap();
        let h1 = ensure_password(&conn, &file).await.unwrap();
        assert!(verify_password("first-password-123", &h1));

        std::fs::write(&file, "second-password-456").unwrap();
        let h2 = ensure_password(&conn, &file).await.unwrap();
        assert!(verify_password("second-password-456", &h2));

        std::fs::remove_file(&file).unwrap();
        let h3 = ensure_password(&conn, &file).await.unwrap();
        assert!(verify_password("second-password-456", &h3), "row is used when file is gone");

        std::fs::write(&file, "short").unwrap();
        assert!(ensure_password(&conn, &file).await.is_err(), "passwords under 12 chars rejected");
    }
}
