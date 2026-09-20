//! Item trader (spec §5.6, §5.7 and amendments §16).

pub mod blend;
pub mod compare;
pub mod controller;
pub mod engine;
pub mod helpers;
pub mod item;
pub mod item_entry;
pub mod lifecycle;
pub mod orders;
pub mod platform;
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

static CONTROLLER: std::sync::OnceLock<Arc<controller::TraderController>> = std::sync::OnceLock::new();

pub fn get() -> Option<Arc<controller::TraderController>> {
    CONTROLLER.get().cloned()
}

/// Creates the controller (never trading), then supervises its monitor loop and the `/me` checks.
pub async fn start(conn: DatabaseConnection) -> Result<(), Error> {
    let platform = Arc::new(platform::LivePlatform::new(conn.clone()));
    let controller = Arc::new(controller::TraderController::new(conn, platform).await?);
    let _ = CONTROLLER.set(controller.clone());
    let delay = std::time::Duration::from_secs(5);
    crate::collector::runner::supervise("Trader:Monitor", delay, move || controller::monitor_loop(controller.clone()));
    crate::collector::runner::supervise("Trader:Session", delay, session::me_check_loop);
    Ok(())
}
