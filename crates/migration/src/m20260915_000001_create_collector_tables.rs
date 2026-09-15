use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const UP: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS sweep_state (
        item_id TEXT PRIMARY KEY,
        slug TEXT NOT NULL,
        active INTEGER NOT NULL DEFAULT 1,
        first_swept_at TEXT,
        last_swept_at TEXT,
        last_attempt_at TEXT,
        expected_interval_s INTEGER NOT NULL DEFAULT 0,
        consecutive_errors INTEGER NOT NULL DEFAULT 0
    )",
    "CREATE INDEX IF NOT EXISTS idx_sweep_state_active_attempt ON sweep_state (active, last_attempt_at)",
    "CREATE TABLE IF NOT EXISTS sweep_summary (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        item_id TEXT NOT NULL,
        sub_type TEXT NOT NULL,
        swept_at TEXT NOT NULL,
        lane TEXT NOT NULL,
        min_sell INTEGER,
        max_buy INTEGER,
        sell_count INTEGER NOT NULL,
        buy_count INTEGER NOT NULL,
        sell_ingame INTEGER NOT NULL,
        buy_ingame INTEGER NOT NULL,
        top_sells TEXT NOT NULL,
        top_buys TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_sweep_summary_item ON sweep_summary (item_id, sub_type, swept_at)",
    "CREATE INDEX IF NOT EXISTS idx_sweep_summary_swept_at ON sweep_summary (swept_at)",
    "CREATE TABLE IF NOT EXISTS sweep_summary_hourly (
        item_id TEXT NOT NULL,
        sub_type TEXT NOT NULL,
        hour TEXT NOT NULL,
        min_sell_min INTEGER,
        min_sell_avg REAL,
        min_sell_max INTEGER,
        max_buy_min INTEGER,
        max_buy_avg REAL,
        max_buy_max INTEGER,
        sell_count_avg REAL NOT NULL,
        buy_count_avg REAL NOT NULL,
        samples INTEGER NOT NULL,
        PRIMARY KEY (item_id, sub_type, hour)
    )",
    "CREATE TABLE IF NOT EXISTS last_seen_orders (
        order_id TEXT PRIMARY KEY,
        item_id TEXT NOT NULL,
        sub_type TEXT NOT NULL,
        side TEXT NOT NULL,
        platinum INTEGER NOT NULL,
        quantity INTEGER NOT NULL,
        user_id TEXT NOT NULL,
        first_seen TEXT NOT NULL,
        updated_at TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_last_seen_item ON last_seen_orders (item_id)",
    "CREATE INDEX IF NOT EXISTS idx_last_seen_user ON last_seen_orders (user_id, item_id, sub_type, side, first_seen)",
    "CREATE TABLE IF NOT EXISTS vanished_orders (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        order_id TEXT NOT NULL,
        item_id TEXT NOT NULL,
        sub_type TEXT NOT NULL,
        side TEXT NOT NULL,
        platinum INTEGER NOT NULL,
        quantity INTEGER NOT NULL,
        user_id TEXT NOT NULL,
        first_seen TEXT NOT NULL,
        vanished_at TEXT NOT NULL,
        gap_seconds INTEGER,
        kind TEXT NOT NULL,
        status TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_vanished_status ON vanished_orders (status, vanished_at)",
    "CREATE INDEX IF NOT EXISTS idx_vanished_item ON vanished_orders (item_id, status, vanished_at)",
    "CREATE INDEX IF NOT EXISTS idx_vanished_user ON vanished_orders (user_id, vanished_at)",
    "CREATE TABLE IF NOT EXISTS item_stats (
        item_id TEXT NOT NULL,
        sub_type TEXT NOT NULL,
        volume REAL NOT NULL,
        avg_price REAL,
        moving_avg REAL,
        profit REAL,
        min_price INTEGER,
        max_price INTEGER,
        median REAL,
        history_days INTEGER NOT NULL,
        warm INTEGER NOT NULL,
        updated_at TEXT NOT NULL,
        PRIMARY KEY (item_id, sub_type)
    )",
    "CREATE TABLE IF NOT EXISTS item_stats_daily (
        item_id TEXT NOT NULL,
        sub_type TEXT NOT NULL,
        day TEXT NOT NULL,
        volume INTEGER NOT NULL,
        median REAL,
        min_price INTEGER,
        max_price INTEGER,
        PRIMARY KEY (item_id, sub_type, day)
    )",
];

const DOWN: &[&str] = &[
    "DROP TABLE IF EXISTS item_stats_daily",
    "DROP TABLE IF EXISTS item_stats",
    "DROP TABLE IF EXISTS vanished_orders",
    "DROP TABLE IF EXISTS last_seen_orders",
    "DROP TABLE IF EXISTS sweep_summary_hourly",
    "DROP TABLE IF EXISTS sweep_summary",
    "DROP TABLE IF EXISTS sweep_state",
];

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
