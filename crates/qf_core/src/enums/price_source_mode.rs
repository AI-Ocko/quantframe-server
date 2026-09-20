use serde::{Deserialize, Serialize};

/// Which statistics feed the trader's price inputs (spec §25 P7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PriceSourceMode {
    /// The collector's probable-trade statistics (§5.5).
    #[default]
    Inferred,
    /// warframe.market closed-trade dailies blended with the collector's live data (§25).
    Closed,
}
