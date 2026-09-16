use serde_json::Value;
use utils::{get_location, Error, SubType};
use wf_market::enums::OrderType;

use crate::app::{ItemSettings, Settings};
use crate::handlers::ItemEntity;
use crate::helper_link::trades::ReviewItem;
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
    collector_health => collector::collector_health {},
    market_item_history => collector::market_item_history { wfm_url: String, sub_type: Option<String>, days: i64 },
    trader_status => trader::trader_status {},
    trader_start => trader::trader_start {},
    trader_stop => trader::trader_stop {},
    trader_set_options => trader::trader_set_options { dry_run: Option<bool>, delete_buy_orders_on_stop: Option<bool> },
    trader_dry_run_log => trader::trader_dry_run_log { page: i64, limit: i64 },
    trader_dry_run_summary => trader::trader_dry_run_summary { days: i64 },
    trader_interesting_items => trader::trader_interesting_items { settings: ItemSettings },
    helper_devices => helper_link::helper_devices {},
    helper_device_create => helper_link::helper_device_create { name: String },
    helper_device_revoke => helper_link::helper_device_revoke { id: i64 },
    helper_trades => helper_link::helper_trades { status: Option<String>, page: i64, limit: i64 },
    helper_trade_apply => helper_link::helper_trade_apply { event_id: String, items: Vec<ReviewItem> },
    helper_trade_ignore => helper_link::helper_trade_ignore { event_id: String },
    log => logs::log { cause: String, component: String, location: String, log_level: String, message: String, context: Option<Value> },
    log_tail => logs::log_tail { limit: i64 },
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
    export_transaction_json => transaction::export_transaction_json { query: TransactionPaginationQueryDto },
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

    #[tokio::test]
    async fn collector_commands_are_routable_and_validate_args() {
        assert!(COMMANDS.contains(&"collector_health"));
        assert!(COMMANDS.contains(&"market_item_history"));
        let bad = dispatch("market_item_history", json!({"wfmUrl": "x"})).await.unwrap();
        assert!(bad.is_err(), "days is required");
    }

    #[tokio::test]
    async fn trader_commands_are_routable_and_validate_args() {
        for name in ["trader_status", "trader_start", "trader_stop", "trader_set_options", "trader_dry_run_log", "trader_dry_run_summary", "trader_interesting_items"] {
            assert!(COMMANDS.contains(&name), "{name}");
        }
        assert!(dispatch("trader_dry_run_log", json!({"page": 1})).await.unwrap().is_err(), "limit is required");
        assert!(dispatch("trader_dry_run_summary", json!({})).await.unwrap().is_err(), "days is required");
        assert!(dispatch("trader_set_options", json!({})).await.is_some(), "all options are optional");
    }

    #[tokio::test]
    async fn log_tail_is_routable_and_validates_args() {
        assert!(COMMANDS.contains(&"log_tail"));
        let bad = dispatch("log_tail", json!({"limit": "many"})).await.unwrap();
        assert!(bad.is_err(), "a non-numeric limit is rejected");
        let ok = dispatch("log_tail", json!({"limit": 5})).await.unwrap().unwrap();
        assert!(ok.as_array().unwrap().len() <= 5);
    }

    #[tokio::test]
    async fn helper_device_commands_are_routable_and_validate_args() {
        for name in ["helper_devices", "helper_device_create", "helper_device_revoke"] {
            assert!(COMMANDS.contains(&name), "{name}");
        }
        assert!(dispatch("helper_device_create", json!({})).await.unwrap().is_err(), "name is required");
        assert!(dispatch("helper_device_revoke", json!({"id": "one"})).await.unwrap().is_err(), "id must be a number");
    }

    #[tokio::test]
    async fn helper_trade_commands_are_routable_and_validate_args() {
        for name in ["helper_trades", "helper_trade_apply", "helper_trade_ignore"] {
            assert!(COMMANDS.contains(&name), "{name}");
        }
        assert!(dispatch("helper_trades", json!({"page": 1})).await.unwrap().is_err(), "limit is required");
        assert!(dispatch("helper_trade_apply", json!({"eventId": "x"})).await.unwrap().is_err(), "items are required");
        assert!(
            dispatch("helper_trade_apply", json!({"eventId": "x", "items": [{"slug": "a", "quantity": "one", "price": 1}]})).await.unwrap().is_err(),
            "quantity must be a number"
        );
        assert!(dispatch("helper_trade_ignore", json!({})).await.unwrap().is_err(), "eventId is required");
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
