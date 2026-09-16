//! Production side effects for the trader controller.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::json;
use service::sea_orm::DatabaseConnection;
use tokio::task::JoinHandle;
use utils::{error, Error, LoggerOptions};
use wf_market::enums::OrderType;

use super::controller::{BoxFuture, Platform, TraderStatus};
use super::engine::{self, EngineExit};
use super::item::ItemTrader;
use super::lifecycle::{LifecycleState, StopReason};
use super::orders::TradeOrders;
use super::session::{self, SessionSnapshot};
use super::TradeContext;
use crate::collector::ts;
use crate::helper_link::presence::{self, HelperSnapshot};
use crate::types::UIEvent;
use crate::utils::modules::states;
use crate::send_event;

pub struct LivePlatform {
    conn: DatabaseConnection,
}

impl LivePlatform {
    pub fn new(conn: DatabaseConnection) -> Self {
        Self { conn }
    }
}

impl Platform for LivePlatform {
    fn session(&self, now: DateTime<Utc>) -> SessionSnapshot {
        session::get().snapshot(now)
    }

    fn helper(&self, now: DateTime<Utc>) -> HelperSnapshot {
        presence::get().snapshot(now)
    }

    fn game_data_loaded(&self) -> bool {
        states::cache_client()
            .ok()
            .and_then(|cache| cache.tradable_item().get_items().ok())
            .is_some_and(|items| !items.is_empty())
    }

    fn auto_delete(&self) -> bool {
        states::try_app_state().is_some_and(|app| app.settings.live_scraper.general.auto_delete)
    }

    fn spawn_engine(&self, dry_run: bool, running: Arc<AtomicBool>) -> JoinHandle<EngineExit> {
        let conn = self.conn.clone();
        tokio::spawn(async move {
            let live = states::try_app_state().map(|app| app.wfm_client);
            let orders = Arc::new(TradeOrders::new(live, Some(conn.clone()), dry_run));
            let just_started = Arc::new(AtomicBool::new(true));
            let trader = Arc::new(ItemTrader::new(running.clone(), just_started.clone()));
            let check = {
                let (conn, orders, trader) = (conn.clone(), orders.clone(), trader.clone());
                move || {
                    let (conn, orders, trader) = (conn.clone(), orders.clone(), trader.clone());
                    async move {
                        let ctx = TradeContext::load(&conn, orders).await?;
                        trader.check(&ctx).await
                    }
                }
            };
            engine::run_loop(running, just_started, orders, check, engine::CYCLE_PAUSE).await
        })
    }

    fn set_status(&self, status: &'static str) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(crate::commands::user::user_set_status(status.to_string()))
    }

    fn delete_live_buy_orders(&self) -> BoxFuture<'_, Result<usize, Error>> {
        Box::pin(async move {
            let app = states::app_state()?;
            let mut deleted = 0;
            for id in app.wfm_client.order().cache_orders().order_ids(OrderType::Buy) {
                match app.wfm_client.order().delete(&id).await {
                    Ok(_) => deleted += 1,
                    Err(e) => error("Trader:Stop", format!("Failed to delete buy order {}: {}", id, e), &LoggerOptions::default()),
                }
            }
            Ok(deleted)
        })
    }

    fn notify_stopped(&self, reason: &StopReason, dry_run: bool, at: DateTime<Utc>) {
        let Some(app) = states::try_app_state() else { return };
        let mode = if dry_run { "dry-run" } else { "live" };
        let variables = HashMap::from([
            ("<REASON>".to_string(), reason.describe()),
            ("<MODE>".to_string(), mode.to_string()),
            ("<TIME>".to_string(), ts(at)),
        ]);
        app.settings.notifications.on_trader_stopped.send(
            &variables,
            Some(json!({"event": "trader_stopped", "reason": reason, "mode": mode, "at": ts(at)})),
        );
    }

    fn token_expiry_alert_due(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        session::get().expiry_alert_due(now)
    }

    fn notify_token_expiring(&self, expires_at: DateTime<Utc>, now: DateTime<Utc>) {
        let days_left = (expires_at - now).num_days().max(0);
        send_event!(
            UIEvent::OnNotify,
            json!({
                "i18n_key": "token_expiring",
                "color": "yellow",
                "type": "warning",
                "values": {"expires_at": ts(expires_at), "days_left": days_left},
                "settings": {"autoClose": false}
            })
        );
        let Some(app) = states::try_app_state() else { return };
        let variables = HashMap::from([
            ("<EXPIRES_AT>".to_string(), ts(expires_at)),
            ("<DAYS_LEFT>".to_string(), days_left.to_string()),
        ]);
        app.settings.notifications.on_token_expiring.send(
            &variables,
            Some(json!({"event": "token_expiring", "expires_at": ts(expires_at), "days_left": days_left})),
        );
    }

    fn broadcast(&self, status: &TraderStatus) {
        send_event!(UIEvent::LifecycleState, json!(status));
        send_event!(UIEvent::UpdateLiveScraperRunningState, json!(status.state == LifecycleState::Trading));
    }
}
