use std::sync::Arc;

use chrono::Utc;
use serde_json::{json, Value};
use utils::{get_location, Error};

use crate::app::ItemSettings;
use crate::trader::controller::{TraderController, TraderStatus};
use crate::trader::lifecycle::StopReason;
use crate::trader::price_source::{get_interesting_items, StatsPriceSource};
use crate::trader::store::{self, DryRunPage, TraderOptions};
use crate::utils::modules::states;
use crate::DATABASE;

fn controller() -> Result<Arc<TraderController>, Error> {
    crate::trader::get().ok_or_else(|| Error::new("Trader:Rpc", "The trader is not initialised yet", get_location!()))
}

pub async fn trader_status() -> Result<TraderStatus, Error> {
    Ok(controller()?.status(Utc::now()).await)
}

pub async fn trader_start() -> Result<TraderStatus, Error> {
    controller()?.start(Utc::now()).await
}

pub async fn trader_stop() -> Result<TraderStatus, Error> {
    controller()?.stop(StopReason::UserStop, Utc::now()).await
}

pub async fn trader_set_options(
    dry_run: Option<bool>,
    delete_buy_orders_on_stop: Option<bool>,
    helper_override: Option<bool>,
) -> Result<TraderOptions, Error> {
    controller()?.set_options(dry_run, delete_buy_orders_on_stop, helper_override).await
}

pub async fn trader_dry_run_log(page: i64, limit: i64) -> Result<DryRunPage, Error> {
    let conn = DATABASE.get().ok_or_else(|| Error::new("Trader:Rpc", "Database is not ready", get_location!()))?;
    store::dry_run_page(conn, page, limit).await
}

pub async fn trader_interesting_items(settings: ItemSettings) -> Result<Vec<Value>, Error> {
    let conn = DATABASE.get().ok_or_else(|| Error::new("Trader:Rpc", "Database is not ready", get_location!()))?;
    let cache = states::cache_client()?;
    let prices = StatsPriceSource::load(conn, &cache).await?;
    Ok(get_interesting_items(&settings, &prices)
        .into_iter()
        .map(|item| {
            let name = cache.tradable_item().get_by(&item.wfm_id).map(|i| i.name).unwrap_or_default();
            let mut value = serde_json::to_value(&item).unwrap_or_default();
            if let Some(object) = value.as_object_mut() {
                object.insert("name".into(), json!(name));
            }
            value
        })
        .collect())
}
