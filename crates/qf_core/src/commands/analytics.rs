//! Trading Analytics RPCs (spec §22).

use chrono::Utc;
use utils::{get_location, Error};

use crate::analytics::{store, Bucket, ItemRow, PartnerRow, StockRow, TimelineRow};
use crate::trader::price_source::StatsPriceSource;
use crate::utils::modules::states;
use crate::DATABASE;

fn database() -> Result<&'static service::sea_orm::DatabaseConnection, Error> {
    DATABASE.get().ok_or_else(|| Error::new("Analytics:Rpc", "Database is not ready", get_location!()))
}

pub async fn analytics_items(from: String, to: String) -> Result<Vec<ItemRow>, Error> {
    store::items(database()?, &from, &to).await
}

pub async fn analytics_stock() -> Result<Vec<StockRow>, Error> {
    let conn = database()?;
    let prices = StatsPriceSource::load(conn, &states::cache_client()?).await?;
    store::stock(conn, &prices, Utc::now()).await
}

pub async fn analytics_partners(from: String, to: String) -> Result<Vec<PartnerRow>, Error> {
    store::partners(database()?, &from, &to).await
}

pub async fn analytics_timeline(from: String, to: String, bucket: Bucket) -> Result<Vec<TimelineRow>, Error> {
    store::timeline(database()?, &from, &to, bucket).await
}
