//! Market Data RPCs (spec §23 M1–M3).

use chrono::Utc;
use utils::{get_location, Error};

use crate::collector::backfill::{self, BackfillStatus};
use crate::collector::market::{self, Movers, OverviewRow, Warmup};
use crate::trader::price_source::all_item_stats;
use crate::utils::modules::states;
use crate::DATABASE;

fn database() -> Result<&'static service::sea_orm::DatabaseConnection, Error> {
    DATABASE.get().ok_or_else(|| Error::new("Market:Rpc", "Database is not ready", get_location!()))
}

/// `(name, slug)` for a cache item id, or None when the item is no longer tradable.
fn name_of() -> Result<impl Fn(&str) -> Option<(String, String)>, Error> {
    let tradable = states::cache_client()?.tradable_item();
    Ok(move |id: &str| tradable.get_by(id).ok().map(|item| (item.name, item.wfm_url)))
}

pub async fn market_overview() -> Result<Vec<OverviewRow>, Error> {
    let stats = all_item_stats(database()?).await?;
    Ok(market::overview(stats, name_of()?))
}

pub async fn market_movers(min_volume: f64) -> Result<Movers, Error> {
    market::movers(database()?, min_volume, name_of()?).await
}

pub async fn market_warmup() -> Result<Warmup, Error> {
    let stats = all_item_stats(database()?).await?;
    Ok(market::warmup(&stats, Utc::now().date_naive()))
}

/// Starts the 90-day statistics import unless it is already running (spec §24 K4).
pub async fn market_backfill_start() -> Result<BackfillStatus, Error> {
    let conn = database()?.clone();
    let items = states::cache_client()?
        .tradable_item()
        .get_items()?
        .into_iter()
        .map(|item| (item.wfm_id, item.wfm_url))
        .collect();
    Ok(backfill::start(conn, items))
}

pub async fn market_backfill_status() -> Result<BackfillStatus, Error> {
    Ok(backfill::status())
}
