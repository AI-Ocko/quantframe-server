use serde::{Deserialize, Serialize};

/// What the trader's `profit` means, and so what the profit threshold filters on (spec §25 P16).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfitBasis {
    /// The collector's live buy/sell spread (§5.5).
    #[default]
    Spread,
    /// Upstream's mean daily closed price range; closed mode only.
    Range,
}
