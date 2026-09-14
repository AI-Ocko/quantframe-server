use chrono::{DateTime, Utc};
use entity::{
    dto::{FinancialGraph, FinancialReport, PaginatedResult, PriceHistory, PriceHistoryVec},
    transaction::TransactionPaginationQueryDto,
};
use serde_json::{json, Value};
use service::TransactionQuery;
use std::{
    fs::{self},
    path::PathBuf,
};
use utils::SubType;
use utils::*;
use wf_market::{enums::OrderType, Authenticated};

use crate::{
    cache::CacheState,
    utils::{ErrorFromExt, OrderListExt, SubTypeExt},
    DATABASE,
};

pub fn get_app_storage_path() -> PathBuf {
    crate::paths::get().data_dir.clone()
}

pub fn get_sounds_path() -> PathBuf {
    crate::paths::get().sounds_dir()
}

pub fn generate_transaction_summary(
    transactions: &Vec<entity::transaction::Model>,
    date: DateTime<Utc>,
    group_by1: GroupByDate,
    group_by2: &[GroupByDate],
    _previous: bool,
) -> (FinancialReport, FinancialGraph<i64>) {
    let (start, end) = get_start_end_of(date, group_by1);
    let transactions = filters_by(transactions, |t| {
        t.created_at >= start && t.created_at <= end
    });

    let mut grouped = group_by_date(&transactions, |t| t.created_at, group_by2);

    fill_missing_date_keys(&mut grouped, start, end, group_by2);

    let graph = FinancialGraph::<i64>::from(&grouped, |group| {
        FinancialReport::from(&group.to_vec()).total_profit
    });
    (FinancialReport::from(&transactions), graph)
}

/// Paginate a vector of items
pub fn paginate<T: Clone>(items: &[T], page: i64, per_page: i64) -> PaginatedResult<T> {
    let total_items = items.len() as i64;

    let start = (page.saturating_sub(1)) * per_page;
    let end = (start + per_page).min(total_items);

    let start_usize = start as usize;
    let end_usize = end as usize;

    let page_items = if start < total_items && end > 0 {
        items[start_usize..end_usize].to_vec()
    } else if per_page == -1 {
        items.to_vec()
    } else {
        Vec::new()
    };
    let total_pages = if per_page == -1 {
        1
    } else {
        (total_items as f64 / per_page as f64).ceil() as i64
    };
    PaginatedResult {
        results: page_items,
        page,
        limit: per_page,
        total: total_items,
        total_pages,
    }
}

pub async fn populate_item_market_properties(
    properties: &mut Properties,
    raw: impl Into<String>,
    sub_type: Option<SubType>,
    bought: i64,
    list_price: Option<i64>,
    mut operations: OperationSet,
    order_type: OrderType,
    cache: &CacheState,
    wfm: &wf_market::client::Client<Authenticated>,
) -> Result<(), Error> {
    let conn = DATABASE.get().unwrap();
    let raw = raw.into();
    let wfm_sub_type: wf_market::types::SubType = SubTypeExt::from_entity(sub_type.clone());

    // ---------------- Item Info ----------------
    let item_info = cache
        .tradable_item()
        .get_by(&raw)
        .map_err(|e| e.with_location(get_location!()))?;

    properties.set_property_value("name", item_info.name.clone());
    properties.set_property_value("image", item_info.icon.clone());
    properties.set_property_value("t_type", item_info.sub_type.clone());

    // ---------------- Order Info ----------------
    let order = wfm
        .order()
        .cache_orders()
        .find_order(&item_info.wfm_id, &wfm_sub_type, order_type);

    let (platinum, order_properties) = if let Some(order) = order {
        let order_operations = order
            .properties
            .get_property_value("operations", OperationSet::new());

        properties.update_property("price_history", |ph: &mut Vec<PriceHistory>| {
            if ph.is_empty() {
                ph.push(PriceHistory::new(
                    order.updated_at.clone(),
                    order.platinum as i64,
                ));
            }
        });
        operations.merge(&order_operations);

        (order.platinum as i64, order.properties.clone())
    } else {
        (
            list_price.unwrap_or(0),
            wf_market::types::Properties::default(),
        )
    };
    // ---------------- Profitability Info ----------------
    if operations.has("ProfitabilityInfo") {
        let potential_profit = platinum - bought;
        let roi = if bought > 0 {
            (potential_profit as f64 / bought as f64) * 100.0
        } else {
            0.0
        };
        properties.set_property_value("roi_percent", roi);
        properties.set_property_value("potential_profit", potential_profit);
    }
    // ---------------- Transaction Info ----------------
    if operations.has("TransactionInfo") {
        let transactions = TransactionQuery::get_all(
            conn,
            TransactionPaginationQueryDto::new(1, -1)
                .set_wfm_id(&item_info.wfm_id)
                .set_sub_type(sub_type.clone()),
        )
        .await
        .map_err(|e| e.with_location(get_location!()))?;

        properties.set_property_value("report", FinancialReport::from(&transactions.results));
        properties.set_property_value("last_transactions", transactions.take_top(5));
    }

    // ---------------- Market Info ----------------
    if operations.has("MarketInfo") && !operations.has("MarketPopulated") {
        let mut orders = wfm
            .order()
            .get_orders_by_item(&item_info.wfm_url)
            .await
            .map_err(|e| {
                Error::from_wfm(
                    "Command::StockItemGetById",
                    "Failed to fetch orders from WFM: {}",
                    e,
                    get_location!(),
                )
            })?;

        orders.filter_by_sub_type(wfm_sub_type.clone(), false);
        orders.filter_user_status(wf_market::enums::StatusType::InGame, false);
        orders.sort_by_platinum();
        orders.apply_item_info(cache)?;

        // Metrics for Highest, Lowest Sell and Buy Prices
        let sell_highest = orders.highest_price(OrderType::Sell);
        let sell_lowest = orders.lowest_price(OrderType::Sell);
        let buy_highest = orders.highest_price(OrderType::Buy);
        let buy_lowest = orders.lowest_price(OrderType::Buy);

        properties.set_property_value("sell_highest_price", sell_highest);
        properties.set_property_value("sell_lowest_price", sell_lowest);
        properties.set_property_value("buy_highest_price", buy_highest);
        properties.set_property_value("buy_lowest_price", buy_lowest);
        properties.set_property_value("supply", orders.sell_orders.len());
        properties.set_property_value("demand", orders.buy_orders.len());

        let spread = sell_lowest - buy_highest;
        properties.set_property_value("spread", spread);

        let spread_pct = if sell_lowest > 0 {
            spread as f64 / sell_lowest as f64 * 100.0
        } else {
            0.0
        };

        properties.set_property_value("spread_percent", spread_pct);
        properties.set_property_value("orders", orders.take_top(5, order_type));
    }
    // ----------------- Market Populated Info -----------------
    if operations.has("MarketPopulated") {
        properties.merge_properties(order_properties.properties, true, true);
    }
    properties.set_property_value("ui_operations", operations.operations.clone());
    Ok(())
}
