use std::collections::HashSet;

use chrono::Utc;
use serde::Serialize;
use utils::{get_location, Error};

use crate::collector::health::{CollectorHealth, HealthTracker};
use crate::collector::history::{load_history, DailyPoint, HourlyPoint};
use crate::collector::stats::ItemStats;
use crate::collector::{runner, store};
use crate::market::limiter;
use crate::utils::modules::states;
use crate::DATABASE;

/// Cold interval assumed for "items behind" while the collector is off.
const DISABLED_COLD_INTERVAL_S: i64 = 3600;

#[derive(Debug, Clone, Serialize)]
pub struct MarketItemHistory {
    pub item_id: String,
    pub name: String,
    pub slug: String,
    pub sub_types: Vec<String>,
    pub sub_type: String,
    pub stats: Option<ItemStats>,
    pub hourly: Vec<HourlyPoint>,
    pub daily: Vec<DailyPoint>,
    pub last_swept_at: Option<String>,
}

fn database() -> Result<&'static service::sea_orm::DatabaseConnection, Error> {
    DATABASE
        .get()
        .ok_or_else(|| Error::new("Collector:Rpc", "Database is not ready", get_location!()))
}

pub async fn collector_health() -> Result<CollectorHealth, Error> {
    let conn = database()?;
    let now = Utc::now();
    match runner::get() {
        Some(collector) => {
            let expected = collector.cold_expected_interval_s().await?;
            let counts = store::item_counts(conn, now, &collector.hot_ids(), runner::HOT_INTERVAL_S, expected).await?;
            Ok(collector.health.snapshot(now, counts, limiter::global().snapshot()))
        }
        None => {
            let counts = store::item_counts(conn, now, &HashSet::new(), runner::HOT_INTERVAL_S, DISABLED_COLD_INTERVAL_S).await?;
            Ok(HealthTracker::default().snapshot(now, counts, limiter::global().snapshot()))
        }
    }
}

pub async fn market_item_history(wfm_url: String, sub_type: Option<String>, days: i64) -> Result<MarketItemHistory, Error> {
    let conn = database()?;
    let item = states::cache_client()?.tradable_item().get_by(&wfm_url)?;
    let history = load_history(conn, &item.wfm_id, sub_type, days.clamp(1, 90), Utc::now()).await?;
    Ok(MarketItemHistory {
        item_id: history.item_id,
        name: item.name,
        slug: item.wfm_url,
        sub_types: history.sub_types,
        sub_type: history.sub_type,
        stats: history.stats,
        hourly: history.hourly,
        daily: history.daily,
        last_swept_at: history.last_swept_at,
    })
}
