use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

// warframe.market closed-trade dailies and the per-item refresh state (spec §25 P1).
const UP: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS closed_stats_daily (
        item_id TEXT NOT NULL,
        sub_type TEXT NOT NULL,
        day TEXT NOT NULL,
        volume INTEGER NOT NULL,
        median REAL,
        min_price INTEGER,
        max_price INTEGER,
        avg_price REAL,
        wa_price REAL,
        PRIMARY KEY (item_id, sub_type, day)
    )",
    "CREATE INDEX IF NOT EXISTS idx_closed_stats_day ON closed_stats_daily (day)",
    "CREATE TABLE IF NOT EXISTS closed_fetch_state (
        item_id TEXT PRIMARY KEY,
        fetched_at TEXT NOT NULL,
        outcome TEXT NOT NULL
    )",
];
const DOWN: &[&str] = &["DROP TABLE IF EXISTS closed_fetch_state", "DROP TABLE IF EXISTS closed_stats_daily"];

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
