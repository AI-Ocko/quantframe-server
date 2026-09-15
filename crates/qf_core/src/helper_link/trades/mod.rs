//! Trade events reported by `qf-helper` (spec §5.8, amendments E1–E9).

pub mod events;

use serde::{Deserialize, Serialize};
use utils::SubType;

pub use qf_log_parser::{RawItem, RawTrade};

/// Body of `POST /helper/trade` (amendment E4).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IncomingTrade {
    pub event_id: String,
    /// RFC 3339, from the helper's clock.
    pub detected_at: String,
    pub trade: RawTrade,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Purchase,
    Sale,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Purchase => "purchase",
            Direction::Sale => "sale",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedItem {
    /// The in-game name as reported.
    pub name: String,
    pub slug: String,
    pub wfm_id: String,
    pub item_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<SubType>,
    pub quantity: i64,
    /// Platinum for the whole line, the way the handlers expect it.
    pub price: i64,
    /// `name`, `override`, `set` or `review` (amendment E5).
    pub matched_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Resolution {
    #[serde(default)]
    pub direction: Option<Direction>,
    #[serde(default)]
    pub platinum: i64,
    #[serde(default)]
    pub items: Vec<ResolvedItem>,
    /// Non-platinum items on the platinum side; recorded, never applied (amendment E6).
    #[serde(default)]
    pub extras: Vec<RawItem>,
}
