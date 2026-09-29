use serde::{Deserialize, Serialize};

/// How buy candidates are ordered before the `max_buy_candidates` cut (spec §25 P26).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateRanking {
    /// `profit × min(volume, RANK_VOLUME_CAP)`, then volume, then uuid.
    #[default]
    ExpectedProfit,
    /// Volume, then uuid: the pre-P26 order, kept for rollback.
    Volume,
}
