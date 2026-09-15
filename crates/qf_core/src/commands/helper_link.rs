use chrono::Utc;
use utils::{get_location, Error};

use crate::db::DatabaseConnection;
use crate::helper_link::keys::{self, CreatedDevice, HelperDevice};
use crate::helper_link::trades::{
    self,
    events::{self, EventPage, HelperEvent},
    live::LiveEnv,
    ReviewItem,
};
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

/// Newest first; `status` `None` or empty lists every event (amendment E9).
pub async fn helper_trades(status: Option<String>, page: i64, limit: i64) -> Result<EventPage, Error> {
    events::list(conn()?, status.as_deref().filter(|s| !s.is_empty()), page, limit).await
}

pub async fn helper_trade_apply(event_id: String, items: Vec<ReviewItem>) -> Result<HelperEvent, Error> {
    trades::apply_reviewed(conn()?, &LiveEnv, &event_id, items, Utc::now()).await
}

pub async fn helper_trade_ignore(event_id: String) -> Result<HelperEvent, Error> {
    trades::ignore(conn()?, &event_id, Utc::now()).await
}
