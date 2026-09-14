use std::sync::Mutex;

use entity::{
    dto::*,
    enums::TransactionItemType,
    transaction::{dto::TransactionPaginationQueryDto, *},
};
use serde_json::json;
use service::{TransactionMutation, TransactionQuery};
use utils::{get_location, group_by, info, warning, Error, LoggerOptions};

use crate::DATABASE;

pub async fn get_transaction_pagination(
    query: TransactionPaginationQueryDto,
) -> Result<PaginatedResult<transaction::Model>, Error> {
    let conn = DATABASE.get().unwrap();
    match TransactionQuery::get_all(conn, query).await {
        Ok(data) => return Ok(data),
        Err(e) => return Err(e.with_location(get_location!())),
    };
}

pub async fn get_transaction_financial_report(
    query: TransactionPaginationQueryDto,
) -> Result<FinancialReport, Error> {
    let items = get_transaction_pagination(query.clone()).await?.results;

    let mut trading_partners = group_by(&items, |item| {
        if item.user_name == "" {
            "Unknown".to_string()
        } else {
            item.user_name.clone()
        }
    });
    // Remove Unknown trading partners
    trading_partners.remove("Unknown");
    let mut trading_partners = trading_partners
        .iter()
        .map(|(name, items)| {
            FinancialReport::from(items).with_properties(json!({
                "user": name,
            }))
        })
        .collect::<Vec<FinancialReport>>();
    trading_partners.sort_by(|a, b| b.total_transactions.cmp(&a.total_transactions));

    let mut report = FinancialReport::from(&items);
    report.properties.set_property_value(
        "trading_partners",
        trading_partners.into_iter().take(10).collect::<Vec<_>>(),
    );
    Ok(report)
}

pub async fn transaction_delete(id: i64) -> Result<transaction::Model, Error> {
    let conn = DATABASE.get().unwrap();

    let item = TransactionQuery::find_by_id(conn, id)
        .await
        .map_err(|e| e.with_location(get_location!()))?;
    if item.is_none() {
        return Err(Error::new(
            "Command::TransactionDelete",
            format!("Transaction with ID {} not found", id),
            get_location!(),
        ));
    }
    let item = item.unwrap();

    match TransactionMutation::delete_by_id(conn, id).await {
        Ok(_) => {}
        Err(e) => return Err(e.with_location(get_location!())),
    }

    Ok(item)
}
pub async fn transaction_delete_bulk(ids: Vec<i64>) -> Result<u64, Error> {
    let conn = DATABASE.get().unwrap();
    let mut deleted_count = 0;
    for id in ids {
        match TransactionMutation::delete_by_id(conn, id).await {
            Ok(e) => {
                info(
                    "Command::TransactionDeleteBulk",
                    format!("Deleted transaction with ID: {}", id),
                    &LoggerOptions::default(),
                );
                deleted_count += e.rows_affected;
            }
            Err(e) => return Err(e.with_location(get_location!())),
        }
    }

    Ok(deleted_count)
}

pub async fn transaction_update(input: UpdateTransaction) -> Result<transaction::Model, Error> {
    let conn = DATABASE.get().unwrap();
    match TransactionMutation::update_by_id(conn, input).await {
        Ok(transaction) => Ok(transaction),
        Err(e) => return Err(e.with_location(get_location!())),
    }
}
