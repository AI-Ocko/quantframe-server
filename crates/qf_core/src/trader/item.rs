use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use entity::{dto::PriceHistory, enums::stock_status::StockStatus};
use serde_json::json;
use utils::{error, get_location, info, warning, Error, LoggerOptions};
use wf_market::{
    enums::{OrderType, StatusType},
    types::{Order, OrderList, OrderWithUser},
};

use super::helpers::*;
use super::item_entry::ItemEntry;
use super::orders::{route_for, Route, WriteMeta};
use super::price_source::{is_disabled, ItemPriceInfo};
use super::TradeContext;
use crate::{cache::types::CacheTradableItem, enums::TradeMode, send_event, types::UIEvent, utils::{OrderListExt, SubTypeExt}};

static COMPONENT: &str = "Trader:Item:";
static LOG_FILE: &str = "trader_item.log";

fn comp(suffix: &str) -> String {
    format!("{}{}", COMPONENT, suffix)
}

pub struct ItemTrader {
    running: Arc<AtomicBool>,
    just_started: Arc<AtomicBool>,
}

impl ItemTrader {
    pub fn new(running: Arc<AtomicBool>, just_started: Arc<AtomicBool>) -> Self {
        Self { running, just_started }
    }

    fn send_event(&self, key: &str, values: Option<serde_json::Value>) {
        send_event!(
            UIEvent::SendLiveScraperMessage,
            json!({"i18nKey": format!("item.{}", key), "values": values})
        );
    }

    fn should_stop(&self, ctx: &TradeContext) -> bool {
        !self.running.load(Ordering::SeqCst) || ctx.banned
    }

    async fn delete_unwanted_orders(&self, ctx: &TradeContext, my_orders: &OrderList<Order>) -> Result<(), Error> {
        let general = &ctx.settings.live_scraper.general;
        if !general.delete_conflicting_orders && !general.auto_delete {
            return Ok(());
        }
        let order_ids = orders_to_delete(&ctx.settings, self.just_started.load(Ordering::SeqCst), my_orders);
        let total = order_ids.len();
        let mut current_index = total;
        let meta = WriteMeta { sub_type: String::new(), reason: "AutoDelete".into() };
        for id in order_ids.iter() {
            if self.should_stop(ctx) {
                warning(comp("Delete"), "Trader is not running or user is banned, stopping deletion.", &LoggerOptions::default());
                break;
            }
            match ctx.orders.delete(id, &meta).await {
                Ok(_) => {
                    info(comp("Delete"), &format!("Deleted order with ID: {} {}/{}", id, current_index, total), &LoggerOptions::default());
                    self.send_event("deleted", Some(json!({"current": current_index, "total": total, "id": id})));
                }
                Err(e) => error(
                    comp("Delete"),
                    &format!("Failed to delete order with ID {}: {}", id, e.message),
                    &LoggerOptions::default().set_file(LOG_FILE),
                ),
            }
            current_index -= 1;
        }
        Ok(())
    }

    pub async fn check(&self, ctx: &TradeContext) -> Result<(), Error> {
        info(comp("Check"), "Checking items...", &LoggerOptions::default());
        let my_orders = ctx.orders.cache_orders();
        self.delete_unwanted_orders(ctx, &my_orders).await?;
        let interesting_items = collect_interesting_items(ctx, COMPONENT).await?;
        self.process_items(interesting_items, ctx).await
    }

    async fn process_items(&self, mut interesting_items: Vec<ItemEntry>, ctx: &TradeContext) -> Result<(), Error> {
        let use_fake = ctx.settings.debugging.live_scraper.fake_orders;
        let mut current_index = 1;
        let existing_buy_order_ids: HashSet<String> =
            ctx.orders.cache_orders().buy_orders.iter().map(|o| o.id.clone()).collect();

        interesting_items.sort_by(|a, b| b.priority.cmp(&a.priority));
        let total = interesting_items.len();

        for item_entry in interesting_items.iter_mut() {
            if self.should_stop(ctx) {
                warning(comp("ProcessItem"), "Trader is not running or user is banned, stopping processing.", &LoggerOptions::default());
                break;
            }
            let item_info = match ctx.cache.tradable_item().get_by(&item_entry.wfm_url) {
                Ok(item) => item,
                Err(e) => {
                    let _ = e.set_component(comp("ProcessItem")).log(LOG_FILE);
                    continue;
                }
            };
            let item_price = ctx.prices.find_by(&item_info.wfm_id, &item_entry.sub_type).unwrap_or_default();
            let route = route_for(ctx.orders.global_dry_run(), item_price.warm);

            self.send_event(
                "checking",
                Some(json!({
                    "current": current_index,
                    "total": total,
                    "name": item_info.name,
                    "sub_type": item_entry.sub_type,
                    "price": item_price
                })),
            );

            let order_path = PathBuf::from(utils::get_base_path())
                .join("fake_orders")
                .join(format!("order_{}.json", item_info.wfm_url));
            let mut orders = load_orders(
                &comp("ProcessItem:LoadOrders:"),
                &ctx.orders,
                &item_entry.wfm_url,
                use_fake.then_some(order_path.as_path()),
            )
            .await?;

            orders.filter_by_sub_type(wf_market::types::SubType::from_entity(item_entry.sub_type.clone()), false);
            orders.filter_username(&ctx.username, true);
            orders.filter_user_status(StatusType::InGame, false);
            orders.sort_by_platinum();
            item_entry.apply_market_info(&orders);

            info(
                &comp("ProcessItem"),
                &format!(
                    "Processing Item: {} | Buy Orders: {} | Sell Orders: {} | Operations: {:?} | Route: {:?} | Progress: {}/{}",
                    item_info.name,
                    orders.buy_orders.len(),
                    orders.sell_orders.len(),
                    item_entry.operations.operations,
                    route,
                    current_index,
                    total
                ),
                &LoggerOptions::default(),
            );

            if item_entry.operations.has("Buy") && !item_entry.operations.has("WishList") {
                progress_buying(ctx, &item_info, item_entry, &item_price, &orders, route)
                    .await
                    .map_err(|e| e.with_location(get_location!()))?;
            }
            if item_entry.operations.has("WishList") {
                progress_wish_list(ctx, &item_info, item_entry, &item_price, &orders, route)
                    .await
                    .map_err(|e| e.with_location(get_location!()))?;
            }
            if item_entry.operations.has("Sell") && item_entry.stock_id.is_some() {
                progress_selling(ctx, &item_info, item_entry, &item_price, &orders, route)
                    .await
                    .map_err(|e| e.with_location(get_location!()))?;
            }
            current_index += 1;
        }

        let all_buy_orders = ctx.orders.cache_orders().extract_order_summary(OrderType::Buy);
        let max_total_price_cap = ctx.settings.live_scraper.items.wtb.max_total_price_cap;
        if all_buy_orders.len() > 1 && !is_disabled(max_total_price_cap) {
            info(
                &comp("GlobalKnapsack"),
                &format!("Running global knapsack check: {} buy orders | Cap: {}", all_buy_orders.len(), max_total_price_cap),
                &LoggerOptions::default(),
            );
            let (_, unselected) = knapsack(all_buy_orders, max_total_price_cap);
            let meta = WriteMeta { sub_type: String::new(), reason: "Knapsack".into() };
            for order in &unselected {
                if order.3.is_empty() || !existing_buy_order_ids.contains(&order.3) {
                    continue;
                }
                if let Err(err) = ctx.orders.delete(&order.3, &meta).await {
                    error(
                        &comp("GlobalKnapsack"),
                        &format!("Failed to delete {}: {}", order.3, err.message),
                        &LoggerOptions::default().set_file(LOG_FILE),
                    );
                }
            }
        }
        Ok(())
    }
}

/// WTB workflow for one item (upstream `progress_buying`).
pub async fn progress_buying(
    ctx: &TradeContext,
    item_info: &CacheTradableItem,
    entry: &mut ItemEntry,
    price: &ItemPriceInfo,
    live_orders: &OrderList<OrderWithUser>,
    route: Route,
) -> Result<(), Error> {
    let conn = &ctx.conn;
    let log_options = &LoggerOptions::default().set_enable(true);
    let component = comp("Buying");
    let settings = &ctx.settings.live_scraper.items;
    let log = |msg: &str| info(&component, msg, log_options);

    if is_blacklisted(settings, item_info, entry, &TradeMode::Buy) {
        log(&format!("Item {} is blacklisted for buying. Skipping.", item_info.name));
        return Ok(());
    }
    let per_trade = get_per_trade(item_info);
    let closed_avg = price.moving_avg.unwrap_or(0.0);
    let max_stock_quantity = settings.wtb.max_stock_quantity;
    let avg_price_cap = settings.wtb.avg_price_cap;
    let max_total_price_cap = settings.wtb.max_total_price_cap;
    let profit_threshold = settings.wtb.profit_threshold;
    let market_info = entry.buy_market_info.clone();
    let mut post_price = market_info.highest_price;
    let (order_id, current_order_price, mut properties, mut trade_operations) =
        get_order_info(entry, OrderType::Buy, &ctx.orders, route);

    if entry.buy_market_info.volume == 0 || entry.sell_market_info.volume == 0 {
        log(&format!("Item {} has no market volume. Skipping WTB order creation.", item_info.name));
        return Ok(());
    }

    if !is_disabled(max_stock_quantity) && entry.stock_id.is_some() {
        let stock_item = entry.get_stock_item_or_error(conn).await?;
        if stock_item.owned >= max_stock_quantity {
            log(&format!(
                "Item {} already has {} units in stock (max: {}). Deleting its WTB order.",
                item_info.name, stock_item.owned, max_stock_quantity
            ));
            delete_order(&component, entry, OrderType::Buy, &ctx.orders, route)
                .await
                .map_err(|e| e.with_location(get_location!()).with_context(entry.to_json()))?;
            return Ok(());
        }
    }

    if let Some(reason) = should_apply_max_price_drop(
        settings.wtb.max_price_drop,
        settings.wtb.min_listings_below,
        current_order_price,
        post_price,
        live_orders.get_price_list(OrderType::Buy, None),
        OrderType::Buy,
    ) {
        post_price = current_order_price;
        trade_operations.add(reason);
    }

    let closed_avg_metric = closed_avg as i64 - post_price;
    let potential_profit = closed_avg_metric - 1;

    let item_max_price = settings.general.get_item_max_price(&item_info.wfm_id);
    if item_max_price > 0 && post_price > item_max_price {
        trade_operations.add("AboveMaxBuyPrice");
        post_price = item_max_price;
    }

    if !is_disabled(avg_price_cap) && post_price > avg_price_cap {
        trade_operations.add("AboveAvgPrice");
        trade_operations.add("Delete");
        log(&format!("Item {} is above the average price cap.", item_info.name));
    }

    if !is_disabled(max_total_price_cap) {
        let mut all_orders = ctx.orders.cache_orders().extract_order_summary(OrderType::Buy);
        if !all_orders.iter().any(|i| i.2 == item_info.wfm_id) {
            all_orders.push((post_price, potential_profit as f64, item_info.wfm_id.clone(), String::new()));
        }
        let (selected, _) = knapsack(all_orders, max_total_price_cap);
        if !selected.iter().any(|o| o.2 == item_info.wfm_id) {
            log(&format!("{} was not selected by the knapsack.", item_info.name));
            return Ok(());
        }
    }

    if closed_avg_metric < 0 {
        trade_operations.add("Delete");
        trade_operations.add("Overpriced");
    }
    if market_info.price_range < profit_threshold {
        trade_operations.add("Delete");
        trade_operations.add("Underpriced");
    }
    post_price = post_price.max(1);

    log_summary(
        &component,
        format!(
            "Item {} | Post: {} | CurOrder: {} ({}) | Market: {} | Price: MovingAvg: {} | Avg: {} | Min: {} | Max: {} | Warm: {} \
             | Profit: Metric: {} | Potential: {} | Threshold: {} | Route: {:?} | Ops: {:?}",
            item_info.name,
            post_price,
            current_order_price,
            order_id,
            market_info,
            closed_avg,
            price.avg_price,
            price.min_price,
            price.max_price,
            price.warm,
            closed_avg_metric,
            potential_profit,
            profit_threshold,
            route,
            trade_operations.operations
        ),
        log_options,
    );

    populate_order_properties(&mut properties, item_info, entry, &trade_operations);
    set_order_market_metrics(&mut properties, post_price, potential_profit, price, live_orders, OrderType::Buy);
    progress_order(
        &component,
        entry,
        &ctx.orders,
        route,
        OrderType::Buy,
        post_price as u32,
        per_trade,
        log_options,
        &mut properties,
        &trade_operations,
    )
    .await
    .map_err(|e| e.with_location(get_location!()).with_context(entry.to_json()))?;
    Ok(())
}

/// WTS workflow for one stock item (upstream `progress_selling`, with the `wts` fix from amendment C5).
pub async fn progress_selling(
    ctx: &TradeContext,
    item_info: &CacheTradableItem,
    entry: &mut ItemEntry,
    price: &ItemPriceInfo,
    live_orders: &OrderList<OrderWithUser>,
    route: Route,
) -> Result<(), Error> {
    let conn = &ctx.conn;
    let log_options = &LoggerOptions::default();
    let component = comp("Selling");
    let settings = &ctx.settings.live_scraper.items;
    let log = |msg: &str| info(&component, msg, log_options);

    if is_blacklisted(settings, item_info, entry, &TradeMode::Sell) {
        log(&format!("Item {} is blacklisted for selling. Skipping.", item_info.name));
        return Ok(());
    }
    let per_trade = get_per_trade(item_info);
    let closed_avg = price.moving_avg.unwrap_or(0.0) as i64;
    let mut stock_item = entry.get_stock_item_or_error(conn).await?;
    let bought_price = stock_item.bought;
    let market_info = entry.sell_market_info.clone();
    let (_, current_order_price, mut properties, mut trade_operations) =
        get_order_info(entry, OrderType::Sell, &ctx.orders, route);

    let (min_price, min_profit, min_sma) = (
        stock_item.properties.get_property_value("min_price", None::<i64>),
        stock_item.properties.get_property_value("min_profit", None::<i64>),
        stock_item.properties.get_property_value("min_sma", None::<i64>),
    );

    if stock_item.is_hidden && stock_item.status == StockStatus::InActive {
        log(&format!("Item {} is marked as hidden and inactive. Skipping.", item_info.name));
        return Ok(());
    } else if stock_item.is_hidden && stock_item.status != StockStatus::InActive {
        stock_item.set_status(StockStatus::InActive);
        stock_item.set_list_price(None);
        stock_item.locked = true;
        trade_operations.add("Delete");
    }

    let lowest_price = if market_info.volume >= 2 {
        market_info.lowest_price
    } else if min_price.is_none() {
        trade_operations.add("Delete");
        trade_operations.add("NoSellers");
        stock_item.set_status(StockStatus::NoSellers);
        stock_item.set_list_price(None);
        stock_item.locked = true;
        0
    } else {
        0
    };

    let mut post_price = lowest_price;
    if let Some(min_price) = min_price {
        let capped_price = post_price.max(min_price);
        if capped_price != post_price {
            post_price = capped_price;
            trade_operations.add("MinimumPrice");
        }
    }

    if let Some(reason) = should_apply_max_price_drop(
        settings.wts.max_price_drop,
        settings.wts.min_listings_below,
        current_order_price,
        post_price,
        live_orders.get_price_list(OrderType::Sell, None),
        OrderType::Sell,
    ) {
        log(&format!("Item {} max price drop applied ({}).", item_info.name, reason));
        post_price = current_order_price;
        trade_operations.add(reason);
    }

    let minimum_sma = min_sma.unwrap_or(settings.wts.min_sma);
    if !is_disabled(minimum_sma) && post_price < (closed_avg - minimum_sma) && lowest_price > bought_price {
        post_price = closed_avg;
        trade_operations.add("SMALimit");
        stock_item.set_list_price(Some(post_price));
        stock_item.set_status(StockStatus::SMALimit);
        stock_item.locked = true;
    }

    let mut profit = post_price - bought_price;
    let minimum_profit = min_profit.unwrap_or(settings.wts.min_profit);
    if !is_disabled(minimum_profit) && profit < minimum_profit {
        post_price += minimum_profit - profit;
        stock_item.set_status(StockStatus::ToLowProfit);
        stock_item.set_list_price(Some(post_price));
        stock_item.locked = true;
        trade_operations.add("LowProfit");
        profit = post_price - bought_price;
    }

    stock_item.set_list_price(Some(post_price));
    stock_item.set_status(StockStatus::Live);
    stock_item.add_price_history(PriceHistory::new(chrono::Local::now().naive_local().to_string(), post_price));
    post_price = post_price.max(1);

    log_summary(
        &component,
        format!(
            "Item {} | Post: {} | CurOrder: {} | Lowest: {} | ClosedAvg: {} | Bought: {} | Profit: {} | Market: {} \
             | Price: AVG: {} | Min: {} | Max: {} | Median: {} | Warm: {} | MinSMA: {} | MinProfit: {} | MinPrice: {:?} \
             | Hidden: {} | Status: {:?} | Route: {:?} | Ops: {:?}",
            item_info.name,
            post_price,
            current_order_price,
            lowest_price,
            closed_avg,
            bought_price,
            profit,
            market_info,
            price.avg_price,
            price.min_price,
            price.max_price,
            price.median,
            price.warm,
            minimum_sma,
            minimum_profit,
            min_price,
            stock_item.is_hidden,
            stock_item.status,
            route,
            trade_operations.operations,
        ),
        log_options,
    );

    populate_order_properties(&mut properties, item_info, entry, &trade_operations);
    set_order_market_metrics(&mut properties, post_price, profit, price, live_orders, OrderType::Sell);
    progress_order(
        &component,
        entry,
        &ctx.orders,
        route,
        OrderType::Sell,
        post_price as u32,
        per_trade,
        log_options,
        &mut properties,
        &trade_operations,
    )
    .await
    .map_err(|e| e.with_location(get_location!()).with_context(entry.to_json()))?;

    entry.finalize_stock_item(conn, &component, &mut stock_item, log_options).await?;
    Ok(())
}

/// Wish-list workflow for one item (upstream `progress_wish_list`).
pub async fn progress_wish_list(
    ctx: &TradeContext,
    item_info: &CacheTradableItem,
    entry: &mut ItemEntry,
    price: &ItemPriceInfo,
    live_orders: &OrderList<OrderWithUser>,
    route: Route,
) -> Result<(), Error> {
    let conn = &ctx.conn;
    let component = comp("WishList:");
    let log_options = LoggerOptions::default();
    let settings = &ctx.settings.live_scraper.items;
    let log = |msg: &str| info(&component, msg, &log_options);

    if is_blacklisted(settings, item_info, entry, &TradeMode::WishList) {
        log(&format!("Item {} is blacklisted for wishlist buying. Skipping.", item_info.name));
        return Ok(());
    }
    let market = entry.buy_market_info.clone();
    let per_trade = get_per_trade(item_info);
    let mut wishlist_item = entry.get_wishlist_item_or_error(conn).await?;
    let (_, _, mut properties, mut trade_operations) = get_order_info(entry, OrderType::Buy, &ctx.orders, route);
    let min_price = wishlist_item.properties.get_property_value("min_price", 0i64);
    let max_price = wishlist_item.properties.get_property_value("max_price", 0i64);

    if wishlist_item.is_hidden {
        if wishlist_item.status == StockStatus::InActive {
            log(&format!("Item {} is marked as hidden and inactive. Skipping.", item_info.name));
            return Ok(());
        }
        wishlist_item.set_status(StockStatus::InActive);
        wishlist_item.set_list_price(None);
        wishlist_item.locked = true;
        trade_operations.add("Delete");
    }

    let mut post_price = if market.volume == 0 {
        trade_operations.add("NoBuyers");
        wishlist_item.set_status(StockStatus::NoBuyers);
        price.avg_price as i64
    } else {
        market.highest_price
    };
    if max_price > 0 && post_price > max_price {
        post_price = max_price;
        trade_operations.add("MaxPrice");
    }
    if min_price > 0 && post_price < min_price {
        post_price = min_price;
        trade_operations.add("MinPrice");
    }
    post_price = post_price.max(1);

    wishlist_item.set_list_price(Some(post_price));
    wishlist_item.set_status(StockStatus::Live);
    wishlist_item.add_price_history(PriceHistory::new(chrono::Local::now().naive_local().to_string(), post_price));

    log_summary(
        &component,
        format!(
            "Item {} | Post: {} | Market: {} | Price: Avg: {} | MovingAvg: {} | Warm: {} | MinPrice: {} | MaxPrice: {} | Route: {:?} | Ops: {:?}",
            item_info.name,
            post_price,
            market,
            price.avg_price,
            price.moving_avg.unwrap_or(0.0),
            price.warm,
            min_price,
            max_price,
            route,
            trade_operations.operations,
        ),
        &log_options,
    );

    populate_order_properties(&mut properties, item_info, entry, &trade_operations);
    set_order_market_metrics(&mut properties, post_price, 0, price, live_orders, OrderType::Buy);
    progress_order(
        &component,
        entry,
        &ctx.orders,
        route,
        OrderType::Buy,
        post_price as u32,
        per_trade,
        &log_options,
        &mut properties,
        &trade_operations,
    )
    .await
    .map_err(|e| e.with_location(get_location!()).with_context(entry.to_json()))?;

    entry.finalize_wishlist_item(conn, &component, &mut wishlist_item, &log_options).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Settings;
    use crate::cache::client::CacheState;
    use crate::trader::orders::TradeOrders;
    use crate::trader::price_source::StatsPriceSource;
    use entity::{stock_item, wish_list};
    use service::{StockItemMutation, WishListMutation};
    use utils::Properties;
    use wf_market::types::{CreateOrderParams, SubType as WFSubType};

    async fn ctx_with(global_dry_run: bool, edit: impl FnOnce(&mut Settings)) -> (tempfile::TempDir, TradeContext) {
        let (dir, conn) = crate::trader::store::tests::db().await;
        let mut settings = Settings::default();
        {
            let items = &mut settings.live_scraper.items;
            items.wtb.max_total_price_cap = -1;
            items.wtb.profit_threshold = -1;
            items.wtb.max_stock_quantity = -1;
            items.wtb.max_price_drop = -1;
            items.wtb.min_listings_below = -1;
            items.wtb.avg_price_cap = -1;
            items.wts.min_sma = -1;
            items.wts.min_profit = -1;
            items.wts.max_price_drop = -1;
            items.wts.min_listings_below = -1;
        }
        edit(&mut settings);
        let ctx = TradeContext {
            conn: conn.clone(),
            cache: CacheState::new(dir.path().to_path_buf()),
            settings,
            orders: Arc::new(TradeOrders::new(None, Some(conn), global_dry_run)),
            prices: Arc::new(StatsPriceSource::default()),
            username: "me".into(),
            banned: false,
        };
        (dir, ctx)
    }

    fn item_info() -> CacheTradableItem {
        CacheTradableItem {
            name: "Test Item".into(),
            unique_name: String::new(),
            wfm_id: "item1".into(),
            wfm_url: "item1_slug".into(),
            trade_tax: 0,
            mr_requirement: 0,
            tags: vec![],
            icon: String::new(),
            bulk_tradable: false,
            sub_type: None,
            variant_to_unique_name: Default::default(),
        }
    }

    fn live_order(side: &str, n: usize, platinum: i64) -> OrderWithUser {
        serde_json::from_value(json!({
            "id": format!("{side}{n}"), "type": side, "platinum": platinum, "quantity": 1, "visible": true,
            "itemId": "item1", "createdAt": "2026-09-01T00:00:00Z", "updatedAt": "2026-09-01T00:00:00Z",
            "user": {"id": format!("u-{side}{n}"), "ingameName": format!("Player{side}{n}"), "reputation": 1, "status": "ingame"}
        }))
        .unwrap()
    }

    fn book(sells: &[i64], buys: &[i64]) -> OrderList<OrderWithUser> {
        let mut orders: Vec<OrderWithUser> = sells.iter().enumerate().map(|(n, p)| live_order("sell", n, *p)).collect();
        orders.extend(buys.iter().enumerate().map(|(n, p)| live_order("buy", n, *p)));
        let mut list = OrderList::new(orders);
        list.sort_by_platinum();
        list
    }

    fn price(moving_avg: f64, warm: bool) -> ItemPriceInfo {
        ItemPriceInfo {
            wfm_id: "item1".into(),
            wfm_url: "item1_slug".into(),
            avg_price: moving_avg,
            moving_avg: Some(moving_avg),
            warm,
            ..Default::default()
        }
    }

    fn entry(ops: &str, stock_id: Option<i64>, wish_list_id: Option<i64>) -> ItemEntry {
        ItemEntry::new(stock_id, wish_list_id, "item1_slug", "item1", None, 0, 1, 1, vec![ops.into()], "closed", Properties::default())
    }

    async fn seed_order(ctx: &TradeContext, order_type: OrderType, platinum: u32, route: Route) {
        let params = CreateOrderParams::new_with_subtype("item1", order_type, platinum, 1, true, None, WFSubType::default());
        ctx.orders.create(params, route, &WriteMeta::default()).await.unwrap();
    }

    async fn stock(ctx: &TradeContext, bought: i64, owned: i64) -> i64 {
        StockItemMutation::create(
            &ctx.conn,
            stock_item::Model::new("item1".into(), "item1_slug".into(), "Test Item".into(), "".into(), None, bought, owned, false, Default::default()),
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn buying_posts_at_the_highest_buy_price_in_global_dry_run() {
        let (_dir, ctx) = ctx_with(true, |_| {}).await;
        let live = book(&[20, 25], &[15, 17]);
        let mut e = entry("Buy", None, None);
        e.apply_market_info(&live);
        progress_buying(&ctx, &item_info(), &mut e, &price(30.0, true), &live, route_for(true, true)).await.unwrap();
        let log = ctx.orders.dry_log();
        assert_eq!(log.len(), 1);
        assert_eq!((log[0].action.as_str(), log[0].side.as_str(), log[0].price, log[0].forced_by.as_str()), ("create", "buy", Some(17), "global"));
    }

    #[tokio::test]
    async fn overpriced_buy_order_is_deleted() {
        let (_dir, ctx) = ctx_with(true, |_| {}).await;
        let route = route_for(true, true);
        seed_order(&ctx, OrderType::Buy, 17, route).await;
        let live = book(&[20, 25], &[15, 17]);
        let mut e = entry("Buy", None, None);
        e.apply_market_info(&live);
        progress_buying(&ctx, &item_info(), &mut e, &price(10.0, true), &live, route).await.unwrap();
        let log = ctx.orders.dry_log();
        assert_eq!(log.last().unwrap().action, "delete");
        assert!(log.last().unwrap().reason.contains("Overpriced"));
    }

    #[tokio::test]
    async fn not_warm_items_are_simulated_even_with_global_dry_run_off() {
        let (_dir, ctx) = ctx_with(false, |_| {}).await;
        let live = book(&[20, 25], &[15, 17]);
        let mut e = entry("Buy", None, None);
        e.apply_market_info(&live);
        let price = price(30.0, false);
        progress_buying(&ctx, &item_info(), &mut e, &price, &live, route_for(false, price.warm)).await.unwrap();
        assert_eq!(ctx.orders.dry_log()[0].forced_by, "not_warm");
    }

    #[tokio::test]
    async fn max_stock_quantity_deletes_the_existing_buy_order() {
        let (_dir, ctx) = ctx_with(true, |s| s.live_scraper.items.wtb.max_stock_quantity = 3).await;
        let route = route_for(true, true);
        let stock_id = stock(&ctx, 10, 5).await;
        seed_order(&ctx, OrderType::Buy, 17, route).await;
        let live = book(&[20, 25], &[15, 17]);
        let mut e = entry("Buy", Some(stock_id), None);
        e.apply_market_info(&live);
        progress_buying(&ctx, &item_info(), &mut e, &price(30.0, true), &live, route).await.unwrap();
        assert_eq!(ctx.orders.dry_log().last().unwrap().action, "delete");
        assert!(ctx.orders.cache_orders().buy_orders.is_empty());
    }

    #[tokio::test]
    async fn selling_max_price_drop_reads_wts_settings() {
        let (_dir, ctx) = ctx_with(true, |s| s.live_scraper.items.wts.max_price_drop = 5).await;
        let route = route_for(true, true);
        let stock_id = stock(&ctx, 10, 1).await;
        seed_order(&ctx, OrderType::Sell, 30, route).await;
        let live = book(&[20, 21], &[5]);
        let mut e = entry("Sell", Some(stock_id), None);
        e.apply_market_info(&live);
        progress_selling(&ctx, &item_info(), &mut e, &price(25.0, true), &live, route).await.unwrap();
        let last = ctx.orders.dry_log().last().unwrap().clone();
        assert_eq!((last.action.as_str(), last.price), ("update", Some(30)));
        assert!(last.reason.contains("MaxPriceDrop"));
    }

    #[tokio::test]
    async fn selling_ignores_wtb_max_price_drop() {
        let (_dir, ctx) = ctx_with(true, |s| s.live_scraper.items.wtb.max_price_drop = 5).await;
        let route = route_for(true, true);
        let stock_id = stock(&ctx, 10, 1).await;
        seed_order(&ctx, OrderType::Sell, 30, route).await;
        let live = book(&[20, 21], &[5]);
        let mut e = entry("Sell", Some(stock_id), None);
        e.apply_market_info(&live);
        progress_selling(&ctx, &item_info(), &mut e, &price(25.0, true), &live, route).await.unwrap();
        let last = ctx.orders.dry_log().last().unwrap().clone();
        assert_eq!((last.action.as_str(), last.price), ("update", Some(20)));
    }

    #[tokio::test]
    async fn wish_list_buy_price_is_capped_by_max_price() {
        let (_dir, ctx) = ctx_with(true, |_| {}).await;
        let wish = WishListMutation::create(
            &ctx.conn,
            &wish_list::Model::new(
                "item1".into(),
                "item1_slug".into(),
                "Test Item".into(),
                "".into(),
                None,
                1,
                Properties::from(json!({"max_price": 16})),
            ),
        )
        .await
        .unwrap();
        let live = book(&[20, 25], &[15, 17]);
        let mut e = entry("WishList", None, Some(wish.id));
        e.apply_market_info(&live);
        progress_wish_list(&ctx, &item_info(), &mut e, &price(30.0, true), &live, route_for(true, true)).await.unwrap();
        let last = ctx.orders.dry_log().last().unwrap().clone();
        assert_eq!((last.action.as_str(), last.price), ("create", Some(16)));
    }
}
