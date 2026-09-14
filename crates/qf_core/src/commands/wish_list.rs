use entity::{dto::*, wish_list::*};
use service::{WishListMutation, WishListQuery};
use std::{collections::HashMap, sync::Mutex};
use utils::SubType;
use utils::{get_location, group_by, info, Error, LoggerOptions, OperationSet};
use wf_market::enums::OrderType;

use crate::{
    app::AppState,
    cache::CacheState,
    handlers::{handle_wfm_item, handle_wish_list, handle_wish_list_by_entity},
    helper,
    DATABASE,
};

pub async fn get_wish_list_pagination(
    query: WishListPaginationQueryDto,
) -> Result<PaginatedResult<Model>, Error> {
    let conn = DATABASE.get().unwrap();
    match WishListQuery::get_all(conn, query).await {
        Ok(data) => return Ok(data),
        Err(e) => return Err(e.with_location(get_location!())),
    };
}

pub async fn get_wish_list_financial_report(
    query: WishListPaginationQueryDto,
) -> Result<FinancialReport, Error> {
    let items = get_wish_list_pagination(query).await?;
    Ok(FinancialReport::from(&items.results))
}

pub async fn get_wish_list_status_counts(
    query: WishListPaginationQueryDto,
) -> Result<HashMap<String, usize>, Error> {
    let items = get_wish_list_pagination(query).await?;
    Ok(group_by(&items.results, |item| item.status.to_string())
        .iter()
        .map(|(status, items)| (status.clone(), items.len()))
        .collect::<HashMap<_, _>>())
}

pub async fn wish_list_create(input: CreateWishListItem) -> Result<Model, Error> {
    match handle_wish_list_by_entity(input, "", OrderType::Sell, &OperationSet::new()).await {
        Ok((_, item)) => return Ok(item),
        Err(e) => {
            return Err(e.with_location(get_location!()).log("wish_list_buy.log"));
        }
    }
}

pub async fn wish_list_bought(
    wfm_url: String,
    sub_type: Option<SubType>,
    quantity: i64,
    price: i64,
) -> Result<Model, Error> {
    match handle_wish_list(
        wfm_url,
        &sub_type,
        quantity,
        price,
        "",
        OrderType::Buy,
        &OperationSet::new(),
    )
    .await
    {
        Ok((_, updated_item)) => return Ok(updated_item),
        Err(e) => {
            return Err(e.with_location(get_location!()).log("wish_list_buy.log"));
        }
    }
}

pub async fn wish_list_delete(id: i64) -> Result<Model, Error> {
    let conn = DATABASE.get().unwrap();

    let item = WishListQuery::get_by_id(conn, id)
        .await
        .map_err(|e| e.with_location(get_location!()))?;
    if item.is_none() {
        return Err(Error::new(
            "Command::WishListDelete",
            format!("Wish list item with ID {} not found", id),
            get_location!(),
        ));
    }
    let item = item.unwrap();

    handle_wfm_item(
        &item.wfm_id,
        &item.sub_type,
        1,
        OrderType::Buy,
        OperationSet::from(vec!["ShouldDelete"]),
    )
    .await
    .map_err(|e| e.with_location(get_location!()).log("wish_list_delete.log"))?;
    match WishListMutation::delete_by_id(conn, id).await {
        Ok(_) => {}
        Err(e) => return Err(e.with_location(get_location!())),
    }

    Ok(item)
}
pub async fn wish_list_delete_multiple(ids: Vec<i64>) -> Result<i64, Error> {
    let conn = DATABASE.get().unwrap();
    let mut deleted_count = 0;

    for id in ids {
        match WishListMutation::delete_by_id(conn, id).await {
            Ok(_) => deleted_count += 1,
            Err(e) => return Err(e.with_location(get_location!())),
        }
    }
    Ok(deleted_count)
}
pub async fn wish_list_update(input: UpdateWishList) -> Result<Model, Error> {
    let conn = DATABASE.get().unwrap();

    match WishListMutation::update_by_id(conn, input).await {
        Ok(item) => Ok(item),
        Err(e) => return Err(e.with_location(get_location!())),
    }
}
pub async fn wish_list_update_multiple(
    ids: Vec<i64>,
    input: UpdateWishList,
) -> Result<Vec<Model>, Error> {
    let conn = DATABASE.get().unwrap();
    let mut updated_items = Vec::new();

    for id in ids {
        let mut update_input = input.clone();
        update_input.id = id;
        match WishListMutation::update_by_id(conn, update_input).await {
            Ok(wish_list) => updated_items.push(wish_list),
            Err(e) => return Err(e.with_location(get_location!())),
        }
    }
    Ok(updated_items)
}

pub async fn wish_list_get_by_id(
    id: i64,
    operations: Option<Vec<String>>) -> Result<wish_list::Model, Error> {
    let cache = crate::utils::modules::states::cache_mutex();
    let app = crate::utils::modules::states::app_mutex();
    let cache = cache.lock()?.clone();
    let app = app.lock()?.clone();
    let conn = DATABASE.get().unwrap();
    let mut item = match WishListQuery::find_by_id(conn, id).await {
        Ok(wish_list_item) => {
            if let Some(item) = wish_list_item {
                item
            } else {
                return Err(Error::new(
                    "Command::WishListGetById",
                    "Wish list item not found",
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
        0,
        item.list_price,
        OperationSet::from(
            operations.unwrap_or(
                vec!["MarketInfo"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
        ),
        OrderType::Buy,
        &cache,
        &app.wfm_client,
    )
    .await?;

    Ok(item)
}

pub async fn export_wish_list_json(
    mut query: WishListPaginationQueryDto,
) -> Result<Vec<wish_list::Model>, Error> {
    let conn = DATABASE.get().unwrap();
    query.pagination.limit = -1; // fetch all
    WishListQuery::get_all(conn, query)
        .await
        .map(|page| page.results)
        .map_err(|e| e.with_location(get_location!()))
}
