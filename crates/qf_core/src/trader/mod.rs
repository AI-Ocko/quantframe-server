//! Item trader (spec §5.6, §5.7 and amendments §16).

pub mod helpers;
pub mod item_entry;
pub mod orders;
pub mod price_source;
pub mod session;
pub mod store;

use std::sync::Arc;

use service::sea_orm::DatabaseConnection;
use utils::Error;

use crate::app::Settings;
use crate::cache::client::CacheState;
use crate::utils::modules::states;
use orders::TradeOrders;
use price_source::{PriceSource, StatsPriceSource};

pub const DRY_RUN_RETENTION_DAYS: i64 = 30;

/// Everything one trader cycle reads. Rebuilt every cycle so settings and prices are current.
pub struct TradeContext {
    pub conn: DatabaseConnection,
    pub cache: CacheState,
    pub settings: Settings,
    pub orders: Arc<TradeOrders>,
    pub prices: Arc<dyn PriceSource>,
    pub username: String,
    pub banned: bool,
}

impl TradeContext {
    pub async fn load(conn: &DatabaseConnection, orders: Arc<TradeOrders>) -> Result<Self, Error> {
        let app = states::app_state()?;
        let cache = states::cache_client()?;
        let prices = StatsPriceSource::load(conn, &cache).await?;
        Ok(Self {
            conn: conn.clone(),
            cache,
            settings: app.settings.clone(),
            orders,
            prices: Arc::new(prices),
            username: app.user.wfm_username.clone(),
            banned: app.user.is_banned(),
        })
    }
}
