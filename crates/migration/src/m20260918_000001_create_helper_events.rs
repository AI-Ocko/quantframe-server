use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const UP: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS helper_events (
        event_id TEXT PRIMARY KEY,
        device_name TEXT NOT NULL,
        received_at TEXT NOT NULL,
        detected_at TEXT NOT NULL,
        status TEXT NOT NULL CHECK (status IN ('applied', 'needs_review', 'ignored')),
        reason TEXT,
        payload TEXT NOT NULL,
        resolution TEXT,
        reviewed_at TEXT
    )",
    "CREATE INDEX IF NOT EXISTS idx_helper_events_status ON helper_events (status, received_at)",
];

const DOWN: &[&str] = &["DROP TABLE IF EXISTS helper_events"];

async fn run(manager: &SchemaManager<'_>, statements: &[&str]) -> Result<(), DbErr> {
    let db = manager.get_connection();
    for sql in statements {
        db.execute(Statement::from_string(db.get_database_backend(), sql.to_string()))
            .await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        run(manager, UP).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        run(manager, DOWN).await
    }
}
