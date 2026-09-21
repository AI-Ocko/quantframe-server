use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

// warframe.market's per-item trade tax, fetched once from the item detail (spec §25 P18).
const UP: &[&str] = &["CREATE TABLE IF NOT EXISTS item_trade_tax (
        item_id TEXT PRIMARY KEY,
        trading_tax INTEGER NOT NULL,
        fetched_at TEXT NOT NULL
    )"];
const DOWN: &[&str] = &["DROP TABLE IF EXISTS item_trade_tax"];

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
