//! Read-only aggregates over the transaction and stock tables (spec §22).

pub mod store;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ItemRow {
    pub wfm_id: String,
    pub wfm_url: String,
    pub item_name: String,
    pub sub_type: String,
    pub purchases: i64,
    pub bought_qty: i64,
    pub spend: i64,
    pub sales: i64,
    pub sold_qty: i64,
    pub revenue: i64,
    pub profit: i64,
    pub avg_buy: Option<f64>,
    pub avg_sell: Option<f64>,
    pub avg_days_held: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PartnerRow {
    pub user_name: String,
    pub trades: i64,
    pub bought_count: i64,
    pub bought_plat: i64,
    pub sold_count: i64,
    pub sold_plat: i64,
    pub profit: i64,
    pub last_trade_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Bucket {
    Day,
    Week,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimelineRow {
    pub bucket_start: String,
    pub sales: i64,
    pub purchases: i64,
    pub revenue: i64,
    pub expenses: i64,
    pub profit: i64,
    pub cumulative_profit: i64,
}
