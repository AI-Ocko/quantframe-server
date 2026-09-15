use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const UP: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS trader_state (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        dry_run INTEGER NOT NULL DEFAULT 1,
        delete_buy_orders_on_stop INTEGER NOT NULL DEFAULT 0,
        helper_override INTEGER NOT NULL DEFAULT 0,
        last_stop_reason TEXT,
        last_stop_at TEXT
    )",
    "INSERT OR IGNORE INTO trader_state (id) VALUES (1)",
    "CREATE TABLE IF NOT EXISTS dry_run_log (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        at TEXT NOT NULL,
        action TEXT NOT NULL,
        side TEXT NOT NULL,
        item_id TEXT NOT NULL,
        sub_type TEXT NOT NULL,
        price INTEGER,
        quantity INTEGER,
        reason TEXT NOT NULL,
        forced_by TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_dry_run_log_at ON dry_run_log (at)",
];

const DOWN: &[&str] = &["DROP TABLE IF EXISTS dry_run_log", "DROP TABLE IF EXISTS trader_state"];

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
