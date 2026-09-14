use std::{collections::HashMap, sync::Mutex};

use entity::{dto::*, stock_item::*};
use service::{StockItemMutation, StockItemQuery};
use utils::{get_location, group_by, info, Error, LoggerOptions, OperationSet, SubType};
use wf_market::enums::OrderType;

use crate::{
    app::AppState,
    cache::CacheState,
    handlers::{handle_item_by_entity, handle_wfm_item, stock_item::handle_item},
    helper::{self},
    DATABASE,
};

pub async fn get_stock_item_pagination(
    query: StockItemPaginationQueryDto,
) -> Result<PaginatedResult<stock_item::Model>, Error> {
    let conn = DATABASE.get().unwrap();
    match StockItemQuery::get_all(conn, query).await {
        Ok(data) => return Ok(data),
        Err(e) => return Err(e.with_location(get_location!())),
    };
}

pub async fn get_stock_item_financial_report(
    query: StockItemPaginationQueryDto,
) -> Result<FinancialReport, Error> {
    let items = get_stock_item_pagination(query).await?;
    Ok(FinancialReport::from(&items.results))
}

pub async fn get_stock_item_status_counts(
    query: StockItemPaginationQueryDto,
) -> Result<HashMap<String, usize>, Error> {
    let items = get_stock_item_pagination(query).await?;
    Ok(group_by(&items.results, |item| item.status.to_string())
        .iter()
        .map(|(status, items)| (status.clone(), items.len()))
        .collect::<HashMap<_, _>>())
}

pub async fn stock_item_create(input: CreateStockItem) -> Result<stock_item::Model, Error> {
    match handle_item_by_entity(input, "", OrderType::Buy, &OperationSet::new()).await {
        Ok((_, updated_item)) => return Ok(updated_item),
        Err(e) => {
            return Err(e
                .with_location(get_location!())
                .log("stock_item_create.log"));
        }
    }
}

pub async fn stock_item_sell(
    wfm_url: String,
    sub_type: Option<SubType>,
    quantity: i64,
    price: i64,
) -> Result<stock_item::Model, Error> {
    match handle_item(
        wfm_url,
        sub_type,
        quantity,
        price,
        "",
        OrderType::Sell,
        &OperationSet::new(),
    )
    .await
    {
        Ok((_, updated_item)) => return Ok(updated_item),
        Err(e) => {
            return Err(e.with_location(get_location!()).log("stock_item_sell.log"));
        }
    }
}

pub async fn stock_item_delete(id: i64) -> Result<stock_item::Model, Error> {
    let conn = DATABASE.get().unwrap();

    let item = StockItemQuery::find_by_id(conn, id)
        .await
        .map_err(|e| e.with_location(get_location!()))?;
    if item.is_none() {
        return Err(Error::new(
            "Command::StockItemDelete",
            format!("Stock item with ID {} not found", id),
            get_location!(),
        ));
    }
    let item = item.unwrap();

    handle_wfm_item(
        &item.wfm_id,
        &item.sub_type,
        1,
        OrderType::Sell,
        OperationSet::from(vec!["ShouldDelete"]),
    )
    .await
    .map_err(|e| {
        e.with_location(get_location!())
            .log("stock_item_delete.log")
    })?;
    match StockItemMutation::delete_by_id(conn, id).await {
        Ok(_) => {}
        Err(e) => return Err(e.with_location(get_location!())),
    }

    Ok(item)
}

pub async fn stock_item_delete_multiple(ids: Vec<i64>) -> Result<i64, Error> {
    let conn = DATABASE.get().unwrap();
    let mut deleted_count = 0;

    for id in ids {
        match StockItemMutation::delete_by_id(conn, id).await {
            Ok(_) => deleted_count += 1,
            Err(e) => return Err(e.with_location(get_location!())),
        }
    }
    Ok(deleted_count)
}

pub async fn stock_item_update(input: UpdateStockItem) -> Result<stock_item::Model, Error> {
    let conn = DATABASE.get().unwrap();
    match StockItemMutation::update_by_id(conn, input).await {
        Ok(stock_item) => Ok(stock_item),
        Err(e) => return Err(e.with_location(get_location!())),
    }
}

pub async fn stock_item_update_multiple(
    ids: Vec<i64>,
    input: UpdateStockItem,
) -> Result<Vec<stock_item::Model>, Error> {
    let conn = DATABASE.get().unwrap();
    let mut updated_items = Vec::new();

    for id in ids {
        let mut update_input = input.clone();
        update_input.id = id;
        match StockItemMutation::update_by_id(conn, update_input).await {
            Ok(stock_item) => updated_items.push(stock_item),
            Err(e) => return Err(e.with_location(get_location!())),
        }
    }
    Ok(updated_items)
}

pub async fn stock_item_get_by_id(
    id: i64,
    operations: Option<Vec<String>>) -> Result<stock_item::Model, Error> {
    let cache = crate::utils::modules::states::cache_mutex();
    let app = crate::utils::modules::states::app_mutex();
    let cache = cache.lock()?.clone();
    let app = app.lock()?.clone();
    let conn = DATABASE.get().unwrap();
    let mut item = match StockItemQuery::find_by_id(conn, id).await {
        Ok(stock_item) => {
            if let Some(item) = stock_item {
                item
            } else {
                return Err(Error::new(
                    "Command::StockItemGetById",
                    "Stock item not found",
                    get_location!(),
                ));
            }
        }
        Err(e) => return Err(e.with_location(get_location!())),
    };

    helper::populate_item_market_properties(
        &mut item.properties,
        &item.wfm_url,
        item.sub_type.clone(),
        item.bought,
        item.list_price,
        OperationSet::from(
            operations.unwrap_or(
                vec!["MarketInfo", "TransactionInfo", "ProfitabilityInfo"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
        ),
        OrderType::Sell,
        &cache,
        &app.wfm_client,
    )
    .await?;

    Ok(item)
}

pub async fn export_stock_item_json(
    mut query: StockItemPaginationQueryDto,
) -> Result<Vec<stock_item::Model>, Error> {
    let conn = DATABASE.get().unwrap();
    query.pagination.limit = -1; // fetch all
    StockItemQuery::get_all(conn, query)
        .await
        .map(|page| page.results)
        .map_err(|e| e.with_location(get_location!()))
}
