use std::sync::Mutex;

use entity::{dto::*, trade_entry::*};
use service::{TradeEntryMutation, TradeEntryQuery};
use utils::{get_location, info, Error, LoggerOptions};

use crate::{utils::CreateTradeEntryExt, DATABASE};

pub async fn get_trade_entry_pagination(
    query: TradeEntryPaginationQueryDto,
) -> Result<PaginatedResult<Model>, Error> {
    let conn = DATABASE.get().unwrap();
    match TradeEntryQuery::get_all(conn, query).await {
        Ok(data) => return Ok(data),
        Err(e) => return Err(e.with_location(get_location!())),
    };
}

pub async fn trade_entry_create(mut input: CreateTradeEntry) -> Result<Model, Error> {
    let conn = DATABASE.get().unwrap();
    input.validate().map_err(|e| {
        let err = e.clone();
        err.with_location(get_location!())
            .log("trade_entry_create.log");
        e
    })?;

    let model = input.to_model();
    match TradeEntryMutation::create_or_update(conn, input.override_existing, &model).await {
        Ok(item) => Ok(item),
        Err(e) => return Err(e.with_location(get_location!())),
    }
}
pub async fn trade_entry_create_multiple(mut inputs: Vec<CreateTradeEntry>) -> Result<i64, Error> {
    let conn = DATABASE.get().unwrap();
    let mut total = 0;
    for input in inputs.iter_mut() {
        input.validate().map_err(|e| {
            let err = e.clone();
            err.with_location(get_location!())
                .log("trade_entry_create_multiple.log");
            e
        })?;
        let model = input.to_model();
        match TradeEntryMutation::create_or_update(conn, input.override_existing, &model).await {
            Ok(_) => total += 1,
            Err(e) => return Err(e.with_location(get_location!())),
        }
    }
    Ok(total)
}

pub async fn trade_entry_delete(id: i64) -> Result<Model, Error> {
    let conn = DATABASE.get().unwrap();

    let item = TradeEntryQuery::get_by_id(conn, id)
        .await
        .map_err(|e| e.with_location(get_location!()))?;
    if item.is_none() {
        return Err(Error::new(
            "Command::TradeEntryDelete",
            format!("Trade entry with ID {} not found", id),
            get_location!(),
        ));
    }
    let item = item.unwrap();
    match TradeEntryMutation::delete_by_id(conn, id).await {
        Ok(_) => {}
        Err(e) => return Err(e.with_location(get_location!())),
    }

    Ok(item)
}
pub async fn trade_entry_delete_multiple(ids: Vec<i64>) -> Result<i64, Error> {
    let conn = DATABASE.get().unwrap();
    let mut deleted_count = 0;

    for id in ids {
        match TradeEntryMutation::delete_by_id(conn, id).await {
            Ok(_) => deleted_count += 1,
            Err(e) => return Err(e.with_location(get_location!())),
        }
    }
    Ok(deleted_count)
}
pub async fn trade_entry_update(input: UpdateTradeEntry) -> Result<Model, Error> {
    let conn = DATABASE.get().unwrap();

    match TradeEntryMutation::update_by_id(conn, input).await {
        Ok(item) => Ok(item),
        Err(e) => return Err(e.with_location(get_location!())),
    }
}
pub async fn trade_entry_get_by_id(id: i64) -> Result<Model, Error> {
    let conn = DATABASE.get().unwrap();

    match TradeEntryQuery::get_by_id(conn, id).await {
        Ok(item) => {
            if let Some(trade_entry) = item {
                Ok(trade_entry)
            } else {
                Err(Error::new(
                    "Command::TradeEntryGetById",
                    format!("Trade entry with ID {} not found", id),
                    get_location!(),
                ))
            }
        }
        Err(e) => return Err(e.with_location(get_location!())),
    }
}
pub async fn trade_entry_update_multiple(
    ids: Vec<i64>,
    input: UpdateTradeEntry,
) -> Result<Vec<Model>, Error> {
    let conn = DATABASE.get().unwrap();
    let mut updated_items = Vec::new();

    for id in ids {
        let mut update_input = input.clone();
        update_input.id = id;
        match TradeEntryMutation::update_by_id(conn, update_input).await {
            Ok(trade_entry) => updated_items.push(trade_entry),
            Err(e) => return Err(e.with_location(get_location!())),
        }
    }
    Ok(updated_items)
}

pub async fn export_trade_entry_json(
    mut query: TradeEntryPaginationQueryDto,
) -> Result<Vec<trade_entry::Model>, Error> {
    let conn = DATABASE.get().unwrap();
    query.pagination.limit = -1; // fetch all
    TradeEntryQuery::get_all(conn, query)
        .await
        .map(|page| page.results)
        .map_err(|e| e.with_location(get_location!()))
}
