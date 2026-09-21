use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use chrono::{DateTime, Utc};

use entity::dto::{add_price_history, PriceHistory};
use entity::stock_item::StockItemPaginationQueryDto;
use entity::wish_list::WishListPaginationQueryDto;
use serde_json::json;
use service::{StockItemQuery, WishListQuery};
use utils::{debug, get_location, info, warning, Error, LoggerOptions, OperationSet};
use wf_market::{
    enums::OrderType,
    types::{CreateOrderParams, Order, OrderList, OrderWithUser, UpdateOrderParams},
};

use super::item_entry::ItemEntry;
use super::orders::{Route, TradeOrders, WriteMeta};
use super::price_source::{get_interesting_items, is_disabled, key_of, ItemPriceInfo};
use super::TradeContext;
use crate::{
    app::{ItemSettings, Settings},
    cache::types::CacheTradableItem,
    enums::TradeMode,
    send_event,
    types::UIEvent,
    utils::SubTypeExt,
};

pub fn knapsack(
    items: Vec<(i64, f64, String, String)>,
    max_weight: i64,
) -> (Vec<(i64, f64, String, String)>, Vec<(i64, f64, String, String)>) {
    let n = items.len();
    let w_max = max_weight.max(0) as usize;
    let mut dp = vec![0.0; w_max + 1];
    let mut choice = vec![vec![false; w_max + 1]; n];
    for (i, item) in items.iter().enumerate() {
        let weight = item.0.max(0) as usize;
        let value = item.1;
        if weight > w_max {
            continue;
        }
        for w in (weight..=w_max).rev() {
            let new_val = dp[w - weight] + value;
            if new_val > dp[w] {
                dp[w] = new_val;
                choice[i][w] = true;
            }
        }
    }
    let mut selected_items = Vec::new();
    let mut unselected_items = Vec::new();
    let mut w = w_max;
    for i in (0..n).rev() {
        let weight = items[i].0.max(0) as usize;
        if w >= weight && choice[i][w] {
            selected_items.push(items[i].clone());
            w -= weight;
        } else {
            unselected_items.push(items[i].clone());
        }
    }
    selected_items.reverse();
    unselected_items.reverse();
    (selected_items, unselected_items)
}

pub async fn collect_interesting_items(ctx: &TradeContext, component: &str) -> Result<Vec<ItemEntry>, Error> {
    let settings = &ctx.settings;
    let conn = &ctx.conn;
    let stock_item_settings = &settings.live_scraper.items;
    let mut interesting_items: HashMap<String, ItemEntry> = HashMap::new();

    if !settings.debugging.live_scraper.entries.is_empty() {
        debug(
            format!("{}Debug", component),
            "Debugging enabled for the trader, using predefined entries",
            &LoggerOptions::default(),
        );
        return serde_json::from_value(serde_json::Value::Array(settings.debugging.live_scraper.entries.clone()))
            .map_err(|e| Error::new(format!("{}Debug", component), format!("Invalid debugging entries: {}", e), get_location!()));
    }

    if settings.live_scraper.has_trade_mode(TradeMode::Buy) {
        for item in get_interesting_items(stock_item_settings, ctx.prices.as_ref()) {
            let item_entry = ItemEntry::from(&item).set_quantity(OrderType::Buy, stock_item_settings.wtb.buy_quantity);
            if !stock_item_settings.general.is_item_blacklisted(&item.wfm_id, &item.sub_type, &TradeMode::Buy) {
                interesting_items.insert(item_entry.uuid(), item_entry);
            }
        }
    }

    if settings.live_scraper.has_trade_mode(TradeMode::Sell) {
        let stock_items = StockItemQuery::get_all(conn, StockItemPaginationQueryDto::new(1, -1))
            .await
            .map_err(|e| e.with_location(get_location!()))?;
        for item in stock_items.results {
            if !stock_item_settings.general.is_item_blacklisted(&item.wfm_id, &item.sub_type, &TradeMode::Sell) {
                interesting_items
                    .entry(item.uuid())
                    .and_modify(|entry| {
                        entry.priority = 1;
                        entry.sell_quantity = item.owned;
                        entry.stock_id = Some(item.id);
                        entry.operations.add("Sell".to_string());
                    })
                    .or_insert_with(|| ItemEntry::from(&item).set_quantity(OrderType::Sell, item.owned));
            }
        }
    }

    if settings.live_scraper.has_trade_mode(TradeMode::WishList) {
        let wish_items = WishListQuery::get_all(conn, WishListPaginationQueryDto::new(1, -1))
            .await
            .map_err(|e| e.with_location(get_location!()))?;
        for item in wish_items.results {
            if !stock_item_settings.general.is_item_blacklisted(&item.wfm_id, &item.sub_type, &TradeMode::WishList) {
                interesting_items
                    .entry(item.uuid())
                    .and_modify(|entry| {
                        entry.priority = 2;
                        entry.buy_quantity = item.quantity;
                        entry.wish_list_id = Some(item.id);
                        entry.operations.add("WishList".to_string());
                    })
                    .or_insert_with(|| ItemEntry::from(&item));
            }
        }
    }
    Ok(interesting_items.into_values().collect())
}

pub fn get_order_info(
    entry: &ItemEntry,
    order_type: OrderType,
    orders: &TradeOrders,
    route: Route,
) -> (String, i64, wf_market::types::Properties, OperationSet) {
    match orders.find_order(&entry.wfm_id, &SubTypeExt::from_entity(entry.sub_type.clone()), order_type, route) {
        None => (String::new(), 0, wf_market::types::Properties::default(), OperationSet::from(vec!["Create"])),
        Some(order) => {
            let mut properties = order.properties;
            properties.set_property_value("id", order.id.clone());
            properties.set_property_value("original_update_string", format!("p:{}", order.platinum));
            (order.id.clone(), i64::from(order.platinum), properties, OperationSet::from(vec!["Update"]))
        }
    }
}

pub fn populate_order_properties(
    properties: &mut wf_market::types::Properties,
    item: &CacheTradableItem,
    entry: &ItemEntry,
    trade_operations: &OperationSet,
) {
    properties.set_property_value("wfm_id", item.wfm_id.clone());
    properties.set_property_value("wfm_url", item.wfm_url.clone());
    properties.set_property_value("name", item.name.clone());
    properties.set_property_value("sub_type", entry.sub_type.clone());
    properties.set_property_value("image", item.icon.clone());
    properties.set_property_value("t_type", item.sub_type.clone());
    let mut operations = entry.operations.clone();
    operations.merge(trade_operations);
    properties.set_property_value("operations", operations);
}

pub fn set_order_market_metrics(
    properties: &mut wf_market::types::Properties,
    post_price: i64,
    profit: i64,
    item_price_info: &ItemPriceInfo,
    live_orders: &OrderList<OrderWithUser>,
    order_type: OrderType,
) {
    let sell_highest = live_orders.highest_price(OrderType::Sell);
    let sell_lowest = live_orders.lowest_price(OrderType::Sell);
    let buy_highest = live_orders.highest_price(OrderType::Buy);
    let buy_lowest = live_orders.lowest_price(OrderType::Buy);
    properties.set_property_value("update_string", format!("p:{}", post_price));
    properties.set_property_value("closed_avg", item_price_info.avg_price);
    properties.set_property_value("potential_profit", profit);
    properties.set_property_value("sell_highest_price", sell_highest);
    properties.set_property_value("sell_lowest_price", sell_lowest);
    properties.set_property_value("buy_highest_price", buy_highest);
    properties.set_property_value("buy_lowest_price", buy_lowest);
    properties.set_property_value("supply", live_orders.sell_orders.len());
    properties.set_property_value("demand", live_orders.buy_orders.len());
    let spread = sell_lowest - buy_highest;
    properties.set_property_value("spread", spread);
    let spread_pct = if sell_lowest > 0 { spread as f64 / sell_lowest as f64 * 100.0 } else { 0.0 };
    properties.set_property_value("spread_percent", spread_pct);
    properties.set_property_value("orders", live_orders.take_top(5, order_type));
    push_price_history(properties, post_price);
}

pub fn push_price_history(properties: &mut wf_market::types::Properties, price: i64) {
    let mut history = properties.get_property_value::<Vec<PriceHistory>>("price_history", vec![]);
    add_price_history(&mut history, PriceHistory::new(chrono::Local::now().naive_local().to_string(), price));
    properties.set_property_value("price_history", history);
}

pub fn orders_to_delete(settings: &Settings, just_started: bool, my_orders: &OrderList<Order>) -> Vec<String> {
    if settings.live_scraper.general.auto_delete && just_started {
        return my_orders
            .to_vec()
            .into_iter()
            .filter(|order| {
                let mode = match order.order_type {
                    OrderType::Buy => TradeMode::Buy,
                    OrderType::Sell => TradeMode::Sell,
                };
                !settings
                    .live_scraper
                    .items
                    .general
                    .is_item_blacklisted(&order.item_id, &SubTypeExt::to_entity(&order.subtype), &mode)
            })
            .map(|order| order.id)
            .collect();
    }
    match (
        settings.live_scraper.has_trade_mode(TradeMode::Buy),
        settings.live_scraper.has_trade_mode(TradeMode::Sell),
        settings.live_scraper.has_trade_mode(TradeMode::WishList),
    ) {
        (true, false, true) => my_orders.order_ids(OrderType::Sell),
        (false, true, false) => my_orders.order_ids(OrderType::Buy),
        _ => vec![],
    }
}

/// How long a buy order must stay uncovered before the sweep deletes it (spec §25 P14).
pub const ORPHAN_GRACE: chrono::Duration = chrono::Duration::minutes(30);

/// Ids of cached buy orders that no `Buy`/`WishList` entry of this cycle covers (spec §25 P14).
pub fn orphan_buy_orders(settings: &Settings, entries: &[ItemEntry], my_orders: &OrderList<Order>) -> Vec<String> {
    if !settings.live_scraper.has_trade_mode(TradeMode::Buy) {
        return Vec::new(); // `orders_to_delete` already removes every buy order in that configuration
    }
    let covered: HashSet<(String, String)> = entries
        .iter()
        .filter(|e| e.operations.has("Buy") || e.operations.has("WishList"))
        .map(|e| (e.wfm_id.clone(), key_of(&e.sub_type)))
        .collect();
    my_orders
        .buy_orders
        .iter()
        .filter(|o| {
            let sub_type = SubTypeExt::to_entity(&o.subtype);
            !covered.contains(&(o.item_id.clone(), key_of(&sub_type)))
                && !settings.live_scraper.items.general.is_item_blacklisted(&o.item_id, &sub_type, &TradeMode::Buy)
        })
        .map(|o| o.id.clone())
        .collect()
}

/// Updates `first_seen` and returns the orphans that have been orphans for at least `grace` (spec §25 P14).
pub fn due_orphans(
    first_seen: &mut HashMap<String, DateTime<Utc>>,
    orphans: &[String],
    now: DateTime<Utc>,
    grace: chrono::Duration,
) -> Vec<String> {
    let current: HashSet<&String> = orphans.iter().collect();
    first_seen.retain(|id, _| current.contains(id));
    orphans
        .iter()
        .filter(|id| now - *first_seen.entry((*id).clone()).or_insert(now) >= grace)
        .cloned()
        .collect()
}

pub async fn load_orders(
    component: &str,
    orders: &TradeOrders,
    item_url: &str,
    fake_path: Option<&Path>,
) -> Result<OrderList<OrderWithUser>, Error> {
    if let Some(path) = fake_path {
        if path.exists() {
            if let Ok(cached) = utils::read_json_file(&path.to_path_buf()) {
                return Ok(cached);
            }
        }
    }
    let live = orders.get_orders_by_item(item_url).await.map_err(|e| e.set_component(component))?;
    if let Some(path) = fake_path {
        utils::write_json_file(path, &live)?;
    }
    Ok(live)
}

#[allow(clippy::too_many_arguments)]
pub async fn progress_order(
    component: &str,
    entry: &ItemEntry,
    orders: &TradeOrders,
    route: Route,
    order_type: OrderType,
    post_price: u32,
    per_trade: Option<i64>,
    log_options: &LoggerOptions,
    properties: &mut wf_market::types::Properties,
    trade_operations: &OperationSet,
) -> Result<OperationSet, Error> {
    let can_create_order = orders.can_create_order(route);
    let quantity = entry.get_quantity(order_type);
    let order_id = properties.get_property_value("id", String::new());
    let name = properties.get_property_value("name", String::new());
    let update_string = properties.get_property_value("update_string", String::new());
    let original_update_string = properties.get_property_value("original_update_string", String::new());
    let meta = WriteMeta { sub_type: key_of(&entry.sub_type), reason: trade_operations.operations.join(",") };

    if trade_operations.has("Create") && !trade_operations.has("Delete") && can_create_order {
        let params = CreateOrderParams::new_with_subtype(
            &entry.wfm_id,
            order_type,
            post_price,
            quantity as u32,
            true,
            per_trade.map(|pt| pt as u32),
            SubTypeExt::from_entity(entry.sub_type.clone()),
        )
        .with_properties(json!(properties.properties));
        let order = orders.create(params, route, &meta).await.map_err(|e| e.with_location(get_location!()))?;
        info(format!("{}CreateSuccess", component), &format!("Created order for item {}: {}", name, order.id), log_options);
        send_event!(UIEvent::RefreshWfmOrders, json!({"source": component}));
    } else if trade_operations.has("Update") && !trade_operations.has("Delete") {
        let params = UpdateOrderParams::new()
            .with_platinum(post_price)
            .with_quantity(quantity as u32)
            .with_per_trade(per_trade.map(|pt| pt as u32))
            .with_properties(json!(properties.properties));
        let order = orders.update(&order_id, params, &meta).await.map_err(|e| e.with_location(get_location!()))?;
        info(format!("{}UpdateSuccess", component), &format!("Updated order for item {}: {}", name, order.id), log_options);
        if original_update_string != update_string {
            send_event!(UIEvent::RefreshWfmOrders, json!({"source": component}));
        }
    } else if trade_operations.has("Update") && trade_operations.has("Delete") {
        let route = orders.delete(&order_id, &meta).await.map_err(|e| e.with_location(get_location!()))?;
        let message = match route {
            Route::DryRun(forced_by) => {
                format!("Simulated delete of order for item {}: {} ({})", name, order_id, forced_by.as_str())
            }
            Route::Live => format!("Deleted order for item {}: {}", name, order_id),
        };
        info(format!("{}DeleteSuccess", component), &message, log_options);
        send_event!(UIEvent::RefreshWfmOrders, json!({"source": component}));
    } else if !can_create_order {
        warning(format!("{}Skip", component), &format!("Item {} has reached the order limit. Skipping.", name), log_options);
    } else {
        warning(format!("{}Skip", component), &format!("Item {} is not optimal for buying. Skipping.", name), log_options);
    }
    Ok(OperationSet::default())
}

/// Deletes this item's existing order, if there is one (amendment C5: upstream never deleted).
pub async fn delete_order(
    component: &str,
    entry: &ItemEntry,
    order_type: OrderType,
    orders: &TradeOrders,
    route: Route,
) -> Result<OperationSet, Error> {
    let (order_id, _, mut properties, _) = get_order_info(entry, order_type, orders, route);
    if order_id.is_empty() {
        return Ok(OperationSet::default());
    }
    let operations = OperationSet::from(vec!["Update", "Delete", "MaxStock"]);
    progress_order(component, entry, orders, route, order_type, 1, None, &LoggerOptions::default(), &mut properties, &operations).await
}

pub fn log_summary(component: &str, message: impl AsRef<str>, options: &LoggerOptions) {
    info(format!("{}Summary", component), message.as_ref(), options);
}

pub fn get_per_trade(item_info: &CacheTradableItem) -> Option<i64> {
    if item_info.bulk_tradable { Some(1) } else { None }
}

pub fn is_blacklisted(settings: &ItemSettings, item_info: &CacheTradableItem, entry: &ItemEntry, mode: &TradeMode) -> bool {
    settings.general.is_item_blacklisted(&item_info.wfm_id, &entry.sub_type, mode)
}

pub fn should_apply_max_price_drop(
    max_price_drop: i64,
    min_listings_below: i64,
    current_order_price: i64,
    post_price: i64,
    prices: Vec<i64>,
    order_type: OrderType,
) -> Option<String> {
    if is_disabled(max_price_drop) && is_disabled(min_listings_below) {
        return None;
    }
    let (is_price_invalid, price_change, listing_count) = match order_type {
        OrderType::Buy => (
            current_order_price > post_price,
            post_price - current_order_price,
            prices.iter().filter(|&&p| p > current_order_price).count() as i64,
        ),
        OrderType::Sell => (
            current_order_price < post_price,
            current_order_price - post_price,
            prices.iter().filter(|&&p| p < current_order_price).count() as i64,
        ),
    };
    if is_price_invalid {
        return None;
    }
    let should_skip = !is_disabled(max_price_drop)
        && price_change > max_price_drop
        && (is_disabled(min_listings_below) || listing_count <= min_listings_below);
    should_skip.then(|| "MaxPriceDrop".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::BlackListItemSetting;
    use utils::{Properties, SubType};
    use wf_market::types::SubType as WFSubType;

    fn order(id: &str, order_type: OrderType, item: &str) -> Order {
        Order {
            id: id.into(),
            order_type,
            platinum: 10,
            quantity: 1,
            per_trade: None,
            subtype: WFSubType::default(),
            visible: true,
            item_id: item.into(),
            created_at: String::new(),
            updated_at: String::new(),
            properties: Default::default(),
        }
    }

    #[test]
    fn knapsack_keeps_the_most_profitable_orders_within_the_cap() {
        let items = vec![
            (60, 10.0, "a".to_string(), "oa".to_string()),
            (50, 30.0, "b".to_string(), "ob".to_string()),
            (50, 25.0, "c".to_string(), "oc".to_string()),
        ];
        let (selected, unselected) = knapsack(items, 100);
        assert_eq!(selected.iter().map(|i| i.2.as_str()).collect::<Vec<_>>(), vec!["b", "c"]);
        assert_eq!(unselected.iter().map(|i| i.2.as_str()).collect::<Vec<_>>(), vec!["a"]);
    }

    #[test]
    fn max_price_drop_holds_the_price_when_it_would_fall_too_far() {
        assert_eq!(should_apply_max_price_drop(-1, -1, 30, 20, vec![20, 21], OrderType::Sell), None);
        assert_eq!(should_apply_max_price_drop(5, -1, 30, 20, vec![20, 21], OrderType::Sell), Some("MaxPriceDrop".into()));
        assert_eq!(should_apply_max_price_drop(15, -1, 30, 20, vec![20, 21], OrderType::Sell), None);
        assert_eq!(should_apply_max_price_drop(5, 1, 30, 20, vec![20, 21], OrderType::Sell), None, "two listings below");
        assert_eq!(should_apply_max_price_drop(5, -1, 10, 20, vec![20], OrderType::Buy), Some("MaxPriceDrop".into()));
    }

    #[test]
    fn orders_to_delete_follows_auto_delete_and_trade_modes() {
        let book = OrderList::new(vec![order("b1", OrderType::Buy, "i1"), order("s1", OrderType::Sell, "i2")]);
        let mut settings = Settings::default();
        settings.live_scraper.general.auto_delete = true;
        let mut all = orders_to_delete(&settings, true, &book);
        all.sort();
        assert_eq!(all, vec!["b1".to_string(), "s1".to_string()]);

        settings.live_scraper.general.trade_modes = vec![TradeMode::Buy, TradeMode::WishList];
        assert_eq!(orders_to_delete(&settings, false, &book), vec!["s1".to_string()]);
        settings.live_scraper.general.trade_modes = vec![TradeMode::Sell];
        assert_eq!(orders_to_delete(&settings, false, &book), vec!["b1".to_string()]);
        settings.live_scraper.general.trade_modes = vec![TradeMode::Buy, TradeMode::Sell, TradeMode::WishList];
        assert!(orders_to_delete(&settings, false, &book).is_empty());
    }

    fn ranked(rank: i64) -> SubType {
        SubType { rank: Some(rank), ..Default::default() }
    }

    fn candidate(item: &str, sub_type: Option<SubType>, operation: &str) -> ItemEntry {
        ItemEntry::new(
            None,
            None,
            format!("{}_slug", item),
            item,
            sub_type,
            0,
            1,
            0,
            vec![operation.into()],
            "closed",
            Properties::default(),
        )
    }

    fn ranked_buy_order(id: &str, item: &str, rank: i64) -> Order {
        Order { subtype: WFSubType { rank: Some(rank), ..Default::default() }, ..order(id, OrderType::Buy, item) }
    }

    #[test]
    fn orphan_buy_orders_returns_only_buy_orders_no_entry_covers() {
        let settings = Settings::default();
        let entries = vec![candidate("i1", None, "Buy"), candidate("i2", None, "WishList"), candidate("i3", None, "Sell")];
        let book = OrderList::new(vec![
            order("b1", OrderType::Buy, "i1"),
            order("b2", OrderType::Buy, "i2"),
            order("b3", OrderType::Buy, "i3"),
            order("b4", OrderType::Buy, "i4"),
            order("s1", OrderType::Sell, "i4"),
        ]);
        let mut orphans = orphan_buy_orders(&settings, &entries, &book);
        orphans.sort();
        assert_eq!(orphans, vec!["b3".to_string(), "b4".to_string()]);
    }

    #[test]
    fn orphan_buy_orders_matches_the_sub_type_and_skips_blacklisted_items() {
        let mut settings = Settings::default();
        settings.live_scraper.items.general.blacklist = vec![
            BlackListItemSetting { wfm_id: "i9".into(), sub_type: None, disabled_for: vec![TradeMode::Buy] },
            BlackListItemSetting { wfm_id: "i8".into(), sub_type: None, disabled_for: vec![TradeMode::Sell] },
        ];
        let entries = vec![candidate("i5", Some(ranked(10)), "Buy")];
        let book = OrderList::new(vec![
            ranked_buy_order("rank10", "i5", 10),
            ranked_buy_order("rank0", "i5", 0),
            order("blacklisted", OrderType::Buy, "i9"),
            order("other_mode", OrderType::Buy, "i8"),
        ]);
        let mut orphans = orphan_buy_orders(&settings, &entries, &book);
        orphans.sort();
        assert_eq!(orphans, vec!["other_mode".to_string(), "rank0".to_string()]);
    }

    #[test]
    fn orphan_buy_orders_is_empty_without_the_buy_trade_mode() {
        let mut settings = Settings::default();
        settings.live_scraper.general.trade_modes = vec![TradeMode::Sell];
        let book = OrderList::new(vec![order("b1", OrderType::Buy, "i1")]);
        assert!(orphan_buy_orders(&settings, &[], &book).is_empty());
    }

    #[test]
    fn due_orphans_waits_out_the_grace_and_forgets_covered_orders() {
        let t0 = DateTime::parse_from_rfc3339("2026-09-20T12:00:00Z").unwrap().with_timezone(&Utc);
        let grace = chrono::Duration::minutes(30);
        let ids = vec!["b1".to_string()];
        let mut first_seen = HashMap::new();

        assert!(due_orphans(&mut first_seen, &ids, t0, grace).is_empty(), "nothing is due on the first sight");
        assert_eq!(first_seen.get("b1"), Some(&t0), "the first sight is recorded");
        assert!(due_orphans(&mut first_seen, &ids, t0 + chrono::Duration::minutes(29), grace).is_empty());
        assert_eq!(due_orphans(&mut first_seen, &ids, t0 + chrono::Duration::minutes(30), grace), vec!["b1".to_string()]);

        assert!(due_orphans(&mut first_seen, &[], t0 + chrono::Duration::minutes(10), grace).is_empty());
        assert!(first_seen.is_empty(), "an order that is covered again is forgotten");
        assert!(due_orphans(&mut first_seen, &ids, t0 + chrono::Duration::minutes(20), grace).is_empty());
        assert!(due_orphans(&mut first_seen, &ids, t0 + chrono::Duration::minutes(45), grace).is_empty(), "the clock restarted");
        assert_eq!(due_orphans(&mut first_seen, &ids, t0 + chrono::Duration::minutes(50), grace), vec!["b1".to_string()]);
    }
}
