use std::path::Path;

use migration::{Migrator, MigratorTrait};
use service::sea_orm::{ConnectionTrait, Database, DatabaseConnection};
use utils::{get_location, info, Error, LoggerOptions};

pub const DB_FILE: &str = "quantframe.sqlite";

pub async fn connect(data_dir: &Path) -> Result<DatabaseConnection, Error> {
    let file = data_dir.join(DB_FILE);
    if file.exists() {
        let backup = data_dir.join(format!("{}_backup", DB_FILE));
        std::fs::copy(&file, &backup).map_err(|e| {
            Error::new("Db:Backup", format!("Failed to back up database: {}", e), get_location!())
        })?;
    }
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let conn = Database::connect(url)
        .await
        .map_err(|e| Error::new("Db:Connect", e.to_string(), get_location!()))?;
    conn.execute_unprepared("PRAGMA journal_mode=WAL;")
        .await
        .map_err(|e| Error::new("Db:Wal", e.to_string(), get_location!()))?;
    Migrator::up(&conn, None)
        .await
        .map_err(|e| Error::new("Db:Migrate", format!("Failed to apply migrations: {}", e), get_location!()))?;
    info("Db:Connect", "Database ready", &LoggerOptions::default());
    Ok(conn)
}
