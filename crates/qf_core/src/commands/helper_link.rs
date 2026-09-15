use chrono::Utc;
use utils::{get_location, Error};

use crate::db::DatabaseConnection;
use crate::helper_link::keys::{self, CreatedDevice, HelperDevice};
use crate::DATABASE;

fn conn() -> Result<&'static DatabaseConnection, Error> {
    DATABASE.get().ok_or_else(|| Error::new("HelperLink:Rpc", "Database is not ready", get_location!()))
}

pub async fn helper_devices() -> Result<Vec<HelperDevice>, Error> {
    keys::list(conn()?).await
}

pub async fn helper_device_create(name: String) -> Result<CreatedDevice, Error> {
    keys::create(conn()?, &name, Utc::now()).await
}

pub async fn helper_device_revoke(id: i64) -> Result<bool, Error> {
    keys::revoke(conn()?, id, Utc::now()).await
}
