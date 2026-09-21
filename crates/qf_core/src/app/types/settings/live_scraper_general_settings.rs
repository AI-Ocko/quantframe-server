use crate::enums::{PriceSourceMode, ProfitBasis, StockMode, TradeMode};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveScraperGeneralSettings {
    pub report_to_wfm: bool,
    pub auto_delete: bool,
    pub auto_trade: bool,
    pub stock_mode: StockMode,
    pub trade_modes: Vec<TradeMode>,
    pub delete_conflicting_orders: bool,
    #[serde(default)]
    pub price_source: PriceSourceMode,
    #[serde(default = "default_fast_drop_guard_pct")]
    pub fast_drop_guard_pct: i64,
    #[serde(default)]
    pub profit_basis: ProfitBasis,
}

fn default_fast_drop_guard_pct() -> i64 {
    10
}

impl Default for LiveScraperGeneralSettings {
    fn default() -> Self {
        Self {
            report_to_wfm: true,
            auto_trade: true,
            auto_delete: true,

            stock_mode: StockMode::All,
            trade_modes: vec![TradeMode::Buy, TradeMode::Sell, TradeMode::WishList],
            delete_conflicting_orders: false,
            price_source: PriceSourceMode::Inferred,
            fast_drop_guard_pct: default_fast_drop_guard_pct(),
            profit_basis: ProfitBasis::Spread,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_saved_before_phase_6d_load_with_the_inferred_source_and_a_ten_percent_guard() {
        let old = r#"{"report_to_wfm":true,"auto_delete":false,"auto_trade":true,"stock_mode":"all","trade_modes":["buy"],"delete_conflicting_orders":false}"#;
        let s: LiveScraperGeneralSettings = serde_json::from_str(old).unwrap();
        assert_eq!((s.price_source, s.fast_drop_guard_pct, s.profit_basis), (PriceSourceMode::Inferred, 10, ProfitBasis::Spread));
        let json = serde_json::to_value(LiveScraperGeneralSettings { price_source: PriceSourceMode::Closed, profit_basis: ProfitBasis::Range, ..Default::default() }).unwrap();
        assert_eq!(json["price_source"], "closed");
        assert_eq!(json["profit_basis"], "range");
    }
}
