use serde_json::Value;
use utils::{get_location, Error, SubType};
use wf_market::enums::OrderType;

use crate::app::Settings;
use crate::handlers::ItemEntity;
use crate::utils::WfmOrderPaginationQueryDto;
use entity::{stock_item::*, trade_entry::*, transaction::*, wish_list::*};

macro_rules! rpc_table {
    ($( $name:ident => $module:ident :: $func:ident { $( $arg:ident : $ty:ty ),* $(,)? } ),* $(,)?) => {
        pub const COMMANDS: &[&str] = &[$( stringify!($name) ),*];

        pub async fn dispatch(name: &str, args: Value) -> Option<Result<Value, Error>> {
            match name {
                $(
                    stringify!($name) => {
                        #[derive(serde::Deserialize)]
                        #[serde(rename_all = "camelCase")]
                        #[allow(dead_code)]
                        struct Args { $( $arg: $ty ),* }
                        let parsed: Args = match serde_json::from_value(args) {
                            Ok(parsed) => parsed,
                            Err(e) => {
                                return Some(Err(Error::new(
                                    "Rpc:Args",
                                    format!("Invalid arguments for {}: {}", name, e),
                                    get_location!(),
                                )))
                            }
                        };
                        let result = crate::commands::$module::$func($( parsed.$arg ),*).await;
                        Some(result.and_then(|value| {
                            serde_json::to_value(value)
                                .map_err(|e| Error::new("Rpc:Serialize", e.to_string(), get_location!()))
                        }))
                    }
                )*
                _ => None,
            }
        }
    };
}

rpc_table! {
    initialized => app::initialized {},
    app_get_app_info => app::app_get_app_info {},
    app_get_settings => app::app_get_settings {},
    app_update_settings => app::app_update_settings { settings: Settings },
    app_accept_tos => app::app_accept_tos { id: String },
    app_notify_reset => app::app_notify_reset { id: String },
    app_get_default_settings => app::app_get_default_settings {},
    auth_me => auth::auth_me {},
    auth_login => auth::auth_login { email: String, password: String },
    auth_logout => auth::auth_logout {},
    user_set_status => user::user_set_status { status: String },
    dashboard_summary => dashboard::dashboard_summary {},
    cache_get_tradable_items => cache::cache_get_tradable_items {},
    cache_get_theme_presets => cache::cache_get_theme_presets {},
    log => logs::log { cause: String, component: String, location: String, log_level: String, message: String, context: Option<Value> },
    get_stock_item_pagination => stock_item::get_stock_item_pagination { query: StockItemPaginationQueryDto },
    get_stock_item_financial_report => stock_item::get_stock_item_financial_report { query: StockItemPaginationQueryDto },
    get_stock_item_status_counts => stock_item::get_stock_item_status_counts { query: StockItemPaginationQueryDto },
    stock_item_create => stock_item::stock_item_create { input: CreateStockItem },
    stock_item_delete => stock_item::stock_item_delete { id: i64 },
    stock_item_sell => stock_item::stock_item_sell { wfm_url: String, sub_type: Option<SubType>, quantity: i64, price: i64 },
    stock_item_update => stock_item::stock_item_update { input: UpdateStockItem },
    stock_item_get_by_id => stock_item::stock_item_get_by_id { id: i64, operations: Option<Vec<String>> },
    stock_item_update_multiple => stock_item::stock_item_update_multiple { ids: Vec<i64>, input: UpdateStockItem },
    stock_item_delete_multiple => stock_item::stock_item_delete_multiple { ids: Vec<i64> },
    export_stock_item_json => stock_item::export_stock_item_json { query: StockItemPaginationQueryDto },
    get_wish_list_pagination => wish_list::get_wish_list_pagination { query: WishListPaginationQueryDto },
    get_wish_list_financial_report => wish_list::get_wish_list_financial_report { query: WishListPaginationQueryDto },
    get_wish_list_status_counts => wish_list::get_wish_list_status_counts { query: WishListPaginationQueryDto },
    wish_list_create => wish_list::wish_list_create { input: CreateWishListItem },
    wish_list_bought => wish_list::wish_list_bought { wfm_url: String, sub_type: Option<SubType>, quantity: i64, price: i64 },
    wish_list_delete => wish_list::wish_list_delete { id: i64 },
    wish_list_update => wish_list::wish_list_update { input: UpdateWishList },
    wish_list_get_by_id => wish_list::wish_list_get_by_id { id: i64, operations: Option<Vec<String>> },
    export_wish_list_json => wish_list::export_wish_list_json { query: WishListPaginationQueryDto },
    wish_list_update_multiple => wish_list::wish_list_update_multiple { ids: Vec<i64>, input: UpdateWishList },
    wish_list_delete_multiple => wish_list::wish_list_delete_multiple { ids: Vec<i64> },
    get_transaction_pagination => transaction::get_transaction_pagination { query: TransactionPaginationQueryDto },
    get_transaction_financial_report => transaction::get_transaction_financial_report { query: TransactionPaginationQueryDto },
    transaction_update => transaction::transaction_update { input: UpdateTransaction },
    transaction_delete => transaction::transaction_delete { id: i64 },
    transaction_delete_bulk => transaction::transaction_delete_bulk { ids: Vec<i64> },
    get_trade_entry_pagination => trade_entry::get_trade_entry_pagination { query: TradeEntryPaginationQueryDto },
    trade_entry_get_by_id => trade_entry::trade_entry_get_by_id { id: i64 },
    trade_entry_create => trade_entry::trade_entry_create { input: CreateTradeEntry },
    trade_entry_create_multiple => trade_entry::trade_entry_create_multiple { inputs: Vec<CreateTradeEntry> },
    trade_entry_delete => trade_entry::trade_entry_delete { id: i64 },
    trade_entry_delete_multiple => trade_entry::trade_entry_delete_multiple { ids: Vec<i64> },
    trade_entry_update => trade_entry::trade_entry_update { input: UpdateTradeEntry },
    trade_entry_update_multiple => trade_entry::trade_entry_update_multiple { ids: Vec<i64>, input: UpdateTradeEntry },
    export_trade_entry_json => trade_entry::export_trade_entry_json { query: TradeEntryPaginationQueryDto },
    get_wfm_orders_pagination => order::get_wfm_orders_pagination { query: WfmOrderPaginationQueryDto },
    get_wfm_orders_status_counts => order::get_wfm_orders_status_counts { query: WfmOrderPaginationQueryDto },
    order_refresh => order::order_refresh {},
    order_delete_all => order::order_delete_all { order_type: Option<OrderType> },
    order_delete_by_id => order::order_delete_by_id { id: String },
    get_wfm_order_by_id => order::get_wfm_order_by_id { id: String, operations: Option<Vec<String>> },
    debug_get_wfm_state => debug::debug_get_wfm_state {},
    sound_get_custom_sounds => sound::sound_get_custom_sounds {},
    sound_add_custom_sound => sound::sound_add_custom_sound { name: String, file_name: String, data_base64: String },
    sound_delete_custom_sound => sound::sound_delete_custom_sound { file_name: String },
    handles_handle_items => handlers::handles_handle_items { items: Vec<ItemEntity> },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn unknown_command_returns_none() {
        assert!(dispatch("does_not_exist", json!({})).await.is_none());
    }

    #[tokio::test]
    async fn no_arg_command_accepts_empty_object() {
        let result = dispatch("initialized", json!({})).await.unwrap().unwrap();
        assert!(result.is_boolean());
    }

    #[tokio::test]
    async fn camel_case_args_are_mapped_and_bad_args_are_errors() {
        let ok = dispatch(
            "log",
            json!({"cause": "c", "component": "Test", "location": "l", "logLevel": "Info", "message": "m"}),
        )
        .await
        .unwrap();
        assert!(ok.is_ok(), "{:?}", ok.err());
        let bad = dispatch("log", json!({"cause": 1})).await.unwrap();
        assert!(bad.is_err());
    }

    #[test]
    fn allowlist_has_no_removed_features() {
        for name in COMMANDS {
            for banned in [
                "riven", "auction", "chat", "analytics", "alert", "syndicate", "wfgdpr",
                "wf_inventory", "live_scraper", "permission", "exit", "calculate_tax",
            ] {
                assert!(!name.contains(banned), "{name} must not be exposed");
            }
        }
    }
}
