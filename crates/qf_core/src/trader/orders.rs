//! Order writes for the trader: live wf-market calls or a simulated book (amendments C4, C6).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use chrono::Utc;
use service::sea_orm::DatabaseConnection;
use serde::Serialize;
use utils::{error, get_location, Error, LogLevel, LoggerOptions};
use wf_market::client::Authenticated;
use wf_market::enums::OrderType;
use wf_market::errors::ApiError;
use wf_market::types::{
    CreateOrderParams, Order, OrderList, OrderWithUser, Properties as WFProperties, SubType as WFSubType,
    UpdateOrderParams,
};
use wf_market::Client;

use super::session;
use super::store::{self, DryRunEntry};
use crate::collector::ts;
use crate::utils::ErrorFromExt;

pub const MAX_CONSECUTIVE_FAILURES: u32 = 5;
const DRY_LOG_MEMORY: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForcedBy {
    Global,
    NotWarm,
}

impl ForcedBy {
    pub fn as_str(self) -> &'static str {
        match self {
            ForcedBy::Global => "global",
            ForcedBy::NotWarm => "not_warm",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Live,
    DryRun(ForcedBy),
}

pub fn route_for(global_dry_run: bool, warm: bool) -> Route {
    if global_dry_run {
        Route::DryRun(ForcedBy::Global)
    } else if !warm {
        Route::DryRun(ForcedBy::NotWarm)
    } else {
        Route::Live
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WriteMeta {
    pub sub_type: String,
    pub reason: String,
}

fn side(order_type: OrderType) -> &'static str {
    match order_type {
        OrderType::Buy => "buy",
        OrderType::Sell => "sell",
    }
}

fn simulated_order(params: &CreateOrderParams) -> Order {
    let now = ts(Utc::now());
    Order {
        id: format!("dry-{}", uuid::Uuid::new_v4()),
        order_type: params.order_type,
        platinum: params.platinum,
        quantity: params.quantity,
        per_trade: params.per_trade.map(|p| p.min(u8::MAX as u32) as u8),
        subtype: params.subtype.clone().unwrap_or_default(),
        visible: params.visible,
        item_id: params.item_id.clone(),
        created_at: now.clone(),
        updated_at: now,
        properties: params.properties.clone().map(WFProperties::from).unwrap_or_default(),
    }
}

pub struct TradeOrders {
    live: Option<Client<Authenticated>>,
    conn: Option<DatabaseConnection>,
    global_dry_run: bool,
    book: Mutex<OrderList<Order>>,
    log: Mutex<Vec<DryRunEntry>>,
    failures: AtomicU32,
}

impl TradeOrders {
    /// Under global dry-run the simulated book starts as a copy of the real cached orders.
    pub fn new(live: Option<Client<Authenticated>>, conn: Option<DatabaseConnection>, global_dry_run: bool) -> Self {
        let book = match (&live, global_dry_run) {
            (Some(client), true) => client.order().cache_orders(),
            _ => OrderList::new(vec![]),
        };
        Self {
            live,
            conn,
            global_dry_run,
            book: Mutex::new(book),
            log: Mutex::new(Vec::new()),
            failures: AtomicU32::new(0),
        }
    }

    pub fn global_dry_run(&self) -> bool {
        self.global_dry_run
    }

    /// Every order the trader reasons about: the simulated book, plus real cached orders when not in global dry-run.
    pub fn cache_orders(&self) -> OrderList<Order> {
        let book = self.book.lock().unwrap().clone();
        match (&self.live, self.global_dry_run) {
            (Some(client), false) => {
                let mut all = client.order().cache_orders().to_vec();
                all.extend(book.to_vec());
                OrderList::new(all)
            }
            _ => book,
        }
    }

    /// Real cached buy orders, used by the stop sequence (amendment C9).
    pub fn live_buy_order_ids(&self) -> Vec<String> {
        self.live
            .as_ref()
            .map(|client| client.order().cache_orders().order_ids(OrderType::Buy))
            .unwrap_or_default()
    }

    pub fn find_order(&self, wfm_id: &str, sub_type: &WFSubType, order_type: OrderType, route: Route) -> Option<Order> {
        match route {
            Route::DryRun(_) => self.book.lock().unwrap().find_order(wfm_id, sub_type, order_type),
            Route::Live => self.live.as_ref()?.order().cache_orders().find_order(wfm_id, sub_type, order_type),
        }
    }

    pub fn can_create_order(&self, route: Route) -> bool {
        match route {
            Route::DryRun(_) => true,
            Route::Live => self.live.as_ref().is_some_and(|client| client.order().can_create_order()),
        }
    }

    pub fn consecutive_failures(&self) -> u32 {
        self.failures.load(Ordering::SeqCst)
    }

    pub fn dry_log(&self) -> Vec<DryRunEntry> {
        self.log.lock().unwrap().clone()
    }

    fn in_book(&self, order_id: &str) -> bool {
        self.global_dry_run || self.book.lock().unwrap().get_by_id(order_id).is_some()
    }

    fn book_forced_by(&self) -> ForcedBy {
        if self.global_dry_run { ForcedBy::Global } else { ForcedBy::NotWarm }
    }

    async fn record(&self, action: &str, order: &Order, price: Option<i64>, quantity: Option<i64>, meta: &WriteMeta, forced_by: ForcedBy) {
        let entry = DryRunEntry {
            id: 0,
            at: ts(Utc::now()),
            action: action.to_string(),
            side: side(order.order_type).to_string(),
            item_id: order.item_id.clone(),
            sub_type: meta.sub_type.clone(),
            price,
            quantity,
            reason: meta.reason.clone(),
            forced_by: forced_by.as_str().to_string(),
        };
        if let Some(conn) = &self.conn {
            if let Err(e) = store::insert_dry_run(conn, &entry).await {
                let _ = e.log("trader_dry_run.log");
            }
        }
        let mut log = self.log.lock().unwrap();
        log.push(entry);
        if log.len() > DRY_LOG_MEMORY {
            let excess = log.len() - DRY_LOG_MEMORY;
            log.drain(0..excess);
        }
    }

    fn live_client(&self) -> Result<&Client<Authenticated>, Error> {
        self.live.as_ref().ok_or_else(|| {
            self.failures.fetch_add(1, Ordering::SeqCst);
            Error::new("Trader:Orders", "No live warframe.market client", get_location!())
        })
    }

    async fn finish_live<T>(&self, action: &str, result: Result<T, ApiError>) -> Result<T, Error> {
        match result {
            Ok(value) => {
                self.failures.store(0, Ordering::SeqCst);
                Ok(value)
            }
            Err(e) => {
                self.failures.fetch_add(1, Ordering::SeqCst);
                let level = match &e {
                    ApiError::OrderLimitExceededSamePrice(_) | ApiError::NotFound(_) | ApiError::OrderLimitExceeded(_) => {
                        self.refresh().await;
                        LogLevel::Warning
                    }
                    ApiError::Unauthorized(_) => {
                        session::get().mark_unauthorized();
                        LogLevel::Error
                    }
                    _ => LogLevel::Error,
                };
                Err(Error::from_wfm(
                    format!("Trader:Orders:{}", action),
                    format!("Failed to {} order", action.to_lowercase()),
                    e,
                    get_location!(),
                )
                .set_log_level(level))
            }
        }
    }

    pub async fn create(&self, params: CreateOrderParams, route: Route, meta: &WriteMeta) -> Result<Order, Error> {
        match route {
            Route::DryRun(forced_by) => {
                let order = simulated_order(&params);
                self.book.lock().unwrap().add(order.clone());
                self.record("create", &order, Some(order.platinum as i64), Some(order.quantity as i64), meta, forced_by)
                    .await;
                Ok(order)
            }
            Route::Live => {
                let client = self.live_client()?;
                let result = client.order().create(params).await;
                self.finish_live("Create", result).await
            }
        }
    }

    pub async fn update(&self, order_id: &str, params: UpdateOrderParams, meta: &WriteMeta) -> Result<Order, Error> {
        if self.in_book(order_id) {
            let price = params.platinum.map(i64::from);
            let quantity = params.quantity.map(i64::from);
            let order = {
                let mut book = self.book.lock().unwrap();
                book.update(order_id, params);
                book.get_by_id(order_id)
            }
            .ok_or_else(|| {
                Error::new("Trader:Orders:Update", format!("Simulated order {} not found", order_id), get_location!())
            })?;
            self.record("update", &order, price, quantity, meta, self.book_forced_by()).await;
            return Ok(order);
        }
        let client = self.live_client()?;
        let result = client.order().update(order_id, params).await;
        self.finish_live("Update", result).await
    }

    /// Deletes an order and reports whether it was a simulated or a real delete.
    pub async fn delete(&self, order_id: &str, meta: &WriteMeta) -> Result<Route, Error> {
        if self.in_book(order_id) {
            let order = {
                let mut book = self.book.lock().unwrap();
                let order = book.get_by_id(order_id);
                book.remove_by_id(order_id);
                order
            };
            let forced_by = self.book_forced_by();
            if let Some(order) = order {
                self.record("delete", &order, None, None, meta, forced_by).await;
            }
            return Ok(Route::DryRun(forced_by));
        }
        let client = self.live_client()?;
        let result = client.order().delete(order_id).await;
        self.finish_live("Delete", result).await.map(|_| Route::Live)
    }

    pub async fn refresh(&self) {
        if let Some(client) = &self.live {
            if let Err(e) = client.order().my_orders().await {
                error("Trader:Orders:Refresh", format!("Failed to refresh orders: {}", e), &LoggerOptions::default());
            }
        }
    }

    pub async fn get_orders_by_item(&self, slug: &str) -> Result<OrderList<OrderWithUser>, Error> {
        let client = self.live.as_ref().ok_or_else(|| {
            Error::new("Trader:Orders:Book", "No warframe.market client to read the order book", get_location!())
        })?;
        client.order().get_orders_by_item(slug).await.map_err(|e| {
            Error::from_wfm("Trader:Orders:Book", format!("Failed to get live orders for item {}", slug), e, get_location!())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(item: &str, order_type: OrderType, platinum: u32) -> CreateOrderParams {
        CreateOrderParams::new_with_subtype(item, order_type, platinum, 1, true, None, WFSubType::default())
    }

    fn meta(reason: &str) -> WriteMeta {
        WriteMeta { sub_type: String::new(), reason: reason.into() }
    }

    #[test]
    fn routing_follows_global_dry_run_then_warm() {
        assert_eq!(route_for(true, true), Route::DryRun(ForcedBy::Global));
        assert_eq!(route_for(true, false), Route::DryRun(ForcedBy::Global));
        assert_eq!(route_for(false, false), Route::DryRun(ForcedBy::NotWarm));
        assert_eq!(route_for(false, true), Route::Live);
    }

    #[tokio::test]
    async fn global_dry_run_mirrors_writes_in_the_book_and_logs_them() {
        let orders = TradeOrders::new(None, None, true);
        let route = route_for(true, true);
        let created = orders.create(params("item1", OrderType::Buy, 17), route, &meta("Create")).await.unwrap();
        assert!(created.id.starts_with("dry-"));
        assert_eq!(orders.cache_orders().buy_orders.len(), 1);
        assert!(orders.find_order("item1", &WFSubType::default(), OrderType::Buy, route).is_some());

        let updated = orders.update(&created.id, UpdateOrderParams::new().with_platinum(20), &meta("Update")).await.unwrap();
        assert_eq!(updated.platinum, 20);
        orders.delete(&created.id, &meta("Update,Delete")).await.unwrap();
        assert!(orders.cache_orders().buy_orders.is_empty());

        let log = orders.dry_log();
        assert_eq!(log.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(), vec!["create", "update", "delete"]);
        assert_eq!(log.iter().map(|e| e.price).collect::<Vec<_>>(), vec![Some(17), Some(20), None]);
        assert!(log.iter().all(|e| e.forced_by == "global" && e.side == "buy" && e.item_id == "item1"));
    }

    #[tokio::test]
    async fn not_warm_items_are_simulated_when_global_dry_run_is_off() {
        let orders = TradeOrders::new(None, None, false);
        let created = orders
            .create(params("item2", OrderType::Sell, 40), route_for(false, false), &meta("Create"))
            .await
            .unwrap();
        orders.update(&created.id, UpdateOrderParams::new().with_platinum(35), &meta("Update")).await.unwrap();
        let log = orders.dry_log();
        assert_eq!(log.len(), 2);
        assert!(log.iter().all(|e| e.forced_by == "not_warm" && e.side == "sell"));
    }

    #[tokio::test]
    async fn delete_reports_the_route_it_took() {
        let global = TradeOrders::new(None, None, true);
        let created = global.create(params("item1", OrderType::Buy, 17), route_for(true, true), &meta("Create")).await.unwrap();
        assert_eq!(global.delete(&created.id, &meta("AutoDelete")).await.unwrap(), Route::DryRun(ForcedBy::Global));
        // Under global dry-run an unknown id is still handled in the book (no live call), and reports Global.
        assert_eq!(global.delete("5f1c2d3e4a5b6c7d8e9f0a1b", &meta("AutoDelete")).await.unwrap(), Route::DryRun(ForcedBy::Global));

        let live_off = TradeOrders::new(None, None, false);
        let created = live_off.create(params("item2", OrderType::Sell, 40), route_for(false, false), &meta("Create")).await.unwrap();
        assert_eq!(live_off.delete(&created.id, &meta("Knapsack")).await.unwrap(), Route::DryRun(ForcedBy::NotWarm));
        // Not in the book and no live client: the live path is taken and fails as before.
        assert!(live_off.delete("5f1c2d3e4a5b6c7d8e9f0a1b", &meta("Knapsack")).await.is_err());
        assert_eq!(live_off.consecutive_failures(), 1);
    }

    #[tokio::test]
    async fn live_failures_are_counted() {
        let orders = TradeOrders::new(None, None, false);
        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            assert!(orders.create(params("item3", OrderType::Buy, 10), Route::Live, &meta("Create")).await.is_err());
        }
        assert_eq!(orders.consecutive_failures(), MAX_CONSECUTIVE_FAILURES);
    }

    #[tokio::test]
    async fn simulated_writes_are_persisted_when_a_database_is_given() {
        let (_dir, conn) = crate::trader::store::tests::db().await;
        let orders = TradeOrders::new(None, Some(conn.clone()), true);
        orders.create(params("item1", OrderType::Buy, 17), route_for(true, true), &meta("Create")).await.unwrap();
        let page = store::dry_run_page(&conn, 1, 10).await.unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.results[0].price, Some(17));
    }
}
