//! Daily database backup (spec §19 H6). Task 8 fills this in.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use service::sea_orm::DatabaseConnection;
use utils::Error;

/// Writes today's backup unless it already exists.
pub async fn run(_conn: &DatabaseConnection, _dir: &Path, _today: NaiveDate) -> Result<Option<PathBuf>, Error> {
    Ok(None)
}
