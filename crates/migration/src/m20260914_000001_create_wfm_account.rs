use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            db.get_database_backend(),
            "CREATE TABLE IF NOT EXISTS wfm_account (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                token_ciphertext BLOB NOT NULL,
                nonce BLOB NOT NULL,
                token_expires_at TEXT,
                wfm_user_id TEXT NOT NULL,
                username TEXT NOT NULL,
                created_at TEXT NOT NULL
            )"
            .to_string(),
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(db.get_database_backend(), "DROP TABLE wfm_account".to_string()))
            .await?;
        Ok(())
    }
}
