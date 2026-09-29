use serde::{Deserialize, Serialize};

use crate::enums::CandidateRanking;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemWtbSettings {
    pub volume_threshold: i64,
    pub profit_threshold: i64,
    pub avg_price_cap: i64,
    pub trading_tax_cap: i64,
    pub max_total_price_cap: i64,
    pub price_shift_threshold: i64,
    pub buy_quantity: i64,
    pub min_wtb_profit_margin: i64,
    pub quantity_per_trade: i64,
    pub max_stock_quantity: i64,
    pub max_price_drop: i64,
    pub min_listings_below: i64,
    /// spec §25 P17: how many buy candidates a cycle works, busiest first; `-1` lifts the limit.
    #[serde(default = "default_max_buy_candidates")]
    pub max_buy_candidates: i64,
    /// spec §25 P26: the order candidates are cut in; pre-P26 bodies get `expected_profit`.
    #[serde(default)]
    pub candidate_ranking: CandidateRanking,
}

fn default_max_buy_candidates() -> i64 {
    crate::trader::price_source::MAX_BUY_CANDIDATES as i64
}

impl Default for ItemWtbSettings {
    fn default() -> Self {
        Self {
            volume_threshold: 15,
            profit_threshold: 10,
            avg_price_cap: 600,
            trading_tax_cap: -1,
            buy_quantity: 1,
            max_total_price_cap: 100000,
            price_shift_threshold: -1,
            min_wtb_profit_margin: -1,
            quantity_per_trade: 1,
            max_stock_quantity: -1,
            max_price_drop: -1,
            min_listings_below: -1,
            max_buy_candidates: default_max_buy_candidates(),
            candidate_ranking: CandidateRanking::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_without_max_buy_candidates_defaults_to_150() {
        let mut body = serde_json::to_value(ItemWtbSettings::default()).unwrap();
        body.as_object_mut().unwrap().remove("max_buy_candidates").expect("the key is serialized");
        let parsed: ItemWtbSettings = serde_json::from_value(body).unwrap();
        assert_eq!(parsed.max_buy_candidates, 150);
    }

    /// spec §25 P26: a body saved before the setting existed ranks by expected profit.
    #[test]
    fn pre_p26_settings_load_expected_profit() {
        let mut body = serde_json::to_value(ItemWtbSettings::default()).unwrap();
        assert_eq!(body["candidate_ranking"], "expected_profit");
        body.as_object_mut().unwrap().remove("candidate_ranking");
        let parsed: ItemWtbSettings = serde_json::from_value(body.clone()).unwrap();
        assert_eq!(parsed.candidate_ranking, CandidateRanking::ExpectedProfit);
        body["candidate_ranking"] = "volume".into();
        let parsed: ItemWtbSettings = serde_json::from_value(body).unwrap();
        assert_eq!(parsed.candidate_ranking, CandidateRanking::Volume);
    }
}
