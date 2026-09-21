//! Trader prices from the collector's `item_stats` (spec §5.5 `PriceSource`, amendment C2).

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::{Error, SubType};

use crate::app::{ItemSettings, Settings};
use crate::cache::client::CacheState;
use crate::collector::closed;
use crate::collector::orders::sub_type_key;
use crate::collector::stats::{ItemStats, StatsConfig};
use crate::collector::{db_err, stmt};
use crate::enums::{PriceSourceMode, ProfitBasis, TradeMode};
use crate::trader::blend::{blend, Effective};
use crate::utils::modules::states;

pub const MAX_BUY_CANDIDATES: usize = 150;

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
pub struct ItemPriceInfo {
    pub wfm_url: String,
    pub wfm_id: String,
    pub uuid: String,
    pub volume: f64,
    pub max_price: f64,
    pub min_price: f64,
    pub avg_price: f64,
    pub moving_avg: Option<f64>,
    pub median: f64,
    pub profit: f64,
    #[serde(default)]
    pub profit_margin: f64,
    #[serde(default)]
    pub trading_tax: i64,
    #[serde(default)]
    pub week_price_shift: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<SubType>,
    #[serde(default)]
    pub warm: bool,
    #[serde(default)]
    pub history_days: i64,
    #[serde(default)]
    pub guarded: bool,
    #[serde(default)]
    pub max_rank: Option<i64>,
}

pub fn is_disabled(value: i64) -> bool {
    value <= -1
}

/// The collector `sub_type` column value for a stock or wish-list sub-type (amendment B4).
pub fn key_of(sub_type: &Option<SubType>) -> String {
    match sub_type {
        None => String::new(),
        Some(s) => sub_type_key(s.rank, s.charges, s.variant.as_deref(), s.amber_stars, s.cyan_stars),
    }
}

pub fn sub_type_from_key(key: &str) -> Option<SubType> {
    if key.is_empty() {
        return None;
    }
    let mut sub_type = SubType::default();
    for part in key.split(';') {
        let Some((name, value)) = part.split_once('=') else { continue };
        match name {
            "rank" => sub_type.rank = value.parse().ok(),
            "charges" => sub_type.charges = value.parse().ok(),
            "subtype" => sub_type.variant = Some(value.to_string()),
            "amber" => sub_type.amber_stars = value.parse().ok(),
            "cyan" => sub_type.cyan_stars = value.parse().ok(),
            _ => {}
        }
    }
    Some(sub_type)
}

pub trait PriceSource: Send + Sync {
    fn find_by(&self, wfm_id: &str, sub_type: &Option<SubType>) -> Option<ItemPriceInfo>;
    fn all(&self) -> Vec<ItemPriceInfo>;
    /// Whether this item's `week_price_shift` is known (closed statistics only).
    fn has_shift(&self, _wfm_id: &str, _sub_type_key: &str) -> bool {
        false
    }
}

#[derive(Default)]
pub struct StatsPriceSource {
    items: HashMap<(String, String), ItemPriceInfo>,
    has_shift: HashSet<(String, String)>,
}

impl StatsPriceSource {
    /// Wraps inferred rows with no shift and no guard; used by the tests and by any caller without closed data.
    pub fn from_stats(stats: Vec<ItemStats>, url_of: impl Fn(&str) -> Option<String>) -> Self {
        Self::from_effective(
            stats.into_iter().map(|stats| Effective { stats, week_price_shift: None, guarded: false, closed: false, inferred: true }).collect(),
            url_of,
        )
    }

    /// Items whose id `url_of` can't resolve (no longer tradable) are skipped.
    pub fn from_effective(rows: Vec<Effective>, url_of: impl Fn(&str) -> Option<String>) -> Self {
        let mut has_shift = HashSet::new();
        let items = rows
            .into_iter()
            .filter_map(|e| {
                let s = e.stats;
                let wfm_url = url_of(&s.item_id)?;
                if e.week_price_shift.is_some() {
                    has_shift.insert((s.item_id.clone(), s.sub_type.clone()));
                }
                let info = ItemPriceInfo {
                    uuid: format!("{}:{}", s.item_id, s.sub_type),
                    wfm_url,
                    wfm_id: s.item_id.clone(),
                    sub_type: sub_type_from_key(&s.sub_type),
                    volume: s.volume,
                    max_price: s.max_price.map(|v| v as f64).unwrap_or(0.0),
                    min_price: s.min_price.map(|v| v as f64).unwrap_or(0.0),
                    avg_price: s.avg_price.unwrap_or(0.0),
                    moving_avg: s.moving_avg,
                    median: s.median.unwrap_or(0.0),
                    profit: s.profit.unwrap_or(0.0),
                    profit_margin: 0.0,
                    trading_tax: 0,
                    week_price_shift: e.week_price_shift.unwrap_or(0.0),
                    warm: s.warm,
                    history_days: s.history_days,
                    guarded: e.guarded,
                    max_rank: None,
                };
                Some(((s.item_id, s.sub_type), info))
            })
            .collect();
        Self { items, has_shift }
    }

    /// Fills `trading_tax` from the tradable cache; items the lookup does not know keep 0.
    pub fn with_trade_tax(mut self, tax_of: impl Fn(&str) -> Option<i64>) -> Self {
        for info in self.items.values_mut() {
            info.trading_tax = tax_of(&info.wfm_id).unwrap_or(0);
        }
        self
    }

    /// Fills `max_rank` from the tradable cache (spec §25 P13); items without ranks keep `None`.
    pub fn with_max_rank(mut self, max_rank_of: impl Fn(&str) -> Option<i64>) -> Self {
        for info in self.items.values_mut() {
            info.max_rank = max_rank_of(&info.wfm_id);
        }
        self
    }

    pub async fn load(conn: &DatabaseConnection, cache: &CacheState) -> Result<Self, Error> {
        let rows = effective_stats(conn, source_settings(), Utc::now()).await?;
        let tradable = cache.tradable_item();
        Ok(Self::from_effective(rows, |id| tradable.get_by(id).ok().map(|item| item.wfm_url))
            .with_trade_tax(|id| tradable.get_by(id).ok().map(|item| item.trade_tax))
            .with_max_rank(|id| tradable.get_by(id).ok().and_then(|item| item.sub_type.and_then(|s| s.max_rank))))
    }
}

/// How the trader reads prices, from the live settings (spec §25 P7, P16).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceSettings {
    pub mode: PriceSourceMode,
    pub guard_pct: i64,
    pub profit_basis: ProfitBasis,
}

/// The live settings; inferred, with the guard off and the spread basis, when the app state is not up yet.
pub fn source_settings() -> SourceSettings {
    states::try_app_state()
        .map(|app| SourceSettings {
            mode: app.settings.live_scraper.general.price_source,
            guard_pct: app.settings.live_scraper.general.fast_drop_guard_pct,
            profit_basis: app.settings.live_scraper.general.profit_basis,
        })
        .unwrap_or(SourceSettings { mode: PriceSourceMode::Inferred, guard_pct: -1, profit_basis: ProfitBasis::Spread })
}

/// The one loader every consumer shares (spec §25 P7).
pub async fn effective_stats(conn: &DatabaseConnection, settings: SourceSettings, now: DateTime<Utc>) -> Result<Vec<Effective>, Error> {
    let inferred = all_item_stats(conn).await?;
    let closed = match settings.mode {
        PriceSourceMode::Inferred => Vec::new(),
        PriceSourceMode::Closed => closed::load_fresh(conn, now, StatsConfig::default().warm_min_trades).await?,
    };
    Ok(blend(inferred, closed, settings.mode, settings.guard_pct, settings.profit_basis, now))
}

impl PriceSource for StatsPriceSource {
    fn find_by(&self, wfm_id: &str, sub_type: &Option<SubType>) -> Option<ItemPriceInfo> {
        self.items.get(&(wfm_id.to_string(), key_of(sub_type))).cloned()
    }

    fn all(&self) -> Vec<ItemPriceInfo> {
        self.items.values().cloned().collect()
    }

    fn has_shift(&self, wfm_id: &str, sub_type_key: &str) -> bool {
        self.has_shift.contains(&(wfm_id.to_string(), sub_type_key.to_string()))
    }
}

pub async fn all_item_stats(conn: &DatabaseConnection) -> Result<Vec<ItemStats>, Error> {
    const C: &str = "Trader:ItemStats";
    conn.query_all(stmt(
        "SELECT item_id, sub_type, volume, avg_price, moving_avg, profit, min_price, max_price, median, history_days, warm, updated_at
         FROM item_stats",
        vec![],
    ))
    .await
    .map_err(|e| db_err(C, e))?
    .iter()
    .map(|r| {
        Ok(ItemStats {
            item_id: r.try_get("", "item_id").map_err(|e| db_err(C, e))?,
            sub_type: r.try_get("", "sub_type").map_err(|e| db_err(C, e))?,
            volume: r.try_get("", "volume").map_err(|e| db_err(C, e))?,
            avg_price: r.try_get("", "avg_price").map_err(|e| db_err(C, e))?,
            moving_avg: r.try_get("", "moving_avg").map_err(|e| db_err(C, e))?,
            profit: r.try_get("", "profit").map_err(|e| db_err(C, e))?,
            min_price: r.try_get("", "min_price").map_err(|e| db_err(C, e))?,
            max_price: r.try_get("", "max_price").map_err(|e| db_err(C, e))?,
            median: r.try_get("", "median").map_err(|e| db_err(C, e))?,
            history_days: r.try_get("", "history_days").map_err(|e| db_err(C, e))?,
            warm: r.try_get::<i64>("", "warm").map_err(|e| db_err(C, e))? != 0,
            updated_at: r.try_get("", "updated_at").map_err(|e| db_err(C, e))?,
        })
    })
    .collect()
}

/// Port of upstream `helpers::get_interesting_items` (amendment C2): the volume, profit and
/// average-price filters apply; by volume descending, at most `MAX_BUY_CANDIDATES`.
pub fn get_interesting_items(settings: &ItemSettings, prices: &dyn PriceSource) -> Vec<ItemPriceInfo> {
    let wtb = &settings.wtb;
    let mut items: Vec<ItemPriceInfo> = prices
        .all()
        .into_iter()
        .filter(|i| is_disabled(wtb.volume_threshold) || i.volume > wtb.volume_threshold as f64)
        .filter(|i| is_disabled(wtb.profit_threshold) || i.profit > wtb.profit_threshold as f64)
        .filter(|i| is_disabled(wtb.avg_price_cap) || i.avg_price <= wtb.avg_price_cap as f64)
        // A shift threshold is meaningfully negative ("drop no more than 5 % a week"), so only the exact -1 sentinel disables it.
        .filter(|i| wtb.price_shift_threshold == -1 || !prices.has_shift(&i.wfm_id, &key_of(&i.sub_type)) || i.week_price_shift >= wtb.price_shift_threshold as f64)
        .filter(|i| is_disabled(wtb.trading_tax_cap) || i.trading_tax <= wtb.trading_tax_cap)
        // Upstream lists mods and arcanes only at their maximum rank (spec §25 P13).
        .filter(|i| match (i.sub_type.as_ref().and_then(|s| s.rank), i.max_rank) {
            (Some(rank), Some(max)) => rank >= max,
            _ => true,
        })
        .collect();
    items.sort_by(|a, b| {
        b.volume
            .partial_cmp(&a.volume)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.uuid.cmp(&b.uuid))
    });
    items.truncate(MAX_BUY_CANDIDATES);
    items
}

/// Buy-candidate item ids for the collector hot set (amendment C3).
pub fn buy_candidate_ids(settings: &Settings, prices: &dyn PriceSource) -> HashSet<String> {
    if !settings.live_scraper.has_trade_mode(TradeMode::Buy) {
        return HashSet::new();
    }
    get_interesting_items(&settings.live_scraper.items, prices)
        .into_iter()
        .map(|i| i.wfm_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(item_id: &str, sub_type: &str, volume: f64, profit: f64, avg: f64) -> ItemStats {
        ItemStats {
            item_id: item_id.into(),
            sub_type: sub_type.into(),
            volume,
            avg_price: Some(avg),
            moving_avg: Some(avg),
            profit: Some(profit),
            min_price: Some(1),
            max_price: Some(999),
            median: Some(avg),
            history_days: 8,
            warm: true,
            updated_at: "2026-09-15T00:00:00Z".into(),
        }
    }

    fn source(rows: Vec<ItemStats>) -> StatsPriceSource {
        StatsPriceSource::from_stats(rows, |id| (id != "gone").then(|| format!("{id}_slug")))
    }

    #[test]
    fn sub_type_keys_round_trip() {
        assert_eq!(key_of(&None), "");
        assert_eq!(sub_type_from_key(""), None);
        for key in ["rank=5", "subtype=intact", "amber=0;cyan=1", "rank=0;charges=3"] {
            assert_eq!(key_of(&sub_type_from_key(key)), key);
        }
        assert_eq!(sub_type_from_key("rank=5").unwrap().rank, Some(5));
    }

    #[test]
    fn stats_map_to_price_info_by_item_and_sub_type() {
        let prices = source(vec![stats("a", "rank=0", 20.0, 15.0, 100.0), stats("a", "rank=5", 2.0, 40.0, 300.0), stats("gone", "", 99.0, 99.0, 1.0)]);
        assert_eq!(prices.all().len(), 2, "untradable items are skipped");
        let rank5 = prices.find_by("a", &sub_type_from_key("rank=5")).unwrap();
        assert_eq!(rank5.wfm_url, "a_slug");
        assert_eq!(rank5.avg_price, 300.0);
        assert_eq!(rank5.max_price, 999.0);
        assert!(rank5.warm);
        assert_eq!(rank5.history_days, 8);
        assert!(prices.find_by("a", &None).is_none());
    }

    #[test]
    fn interesting_items_filter_sort_and_respect_disabled_thresholds() {
        let prices = source(vec![
            stats("a", "", 20.0, 15.0, 100.0),
            stats("b", "", 30.0, 5.0, 100.0),
            stats("c", "", 16.0, 50.0, 700.0),
            stats("d", "", 40.0, 20.0, 50.0),
        ]);
        let mut settings = ItemSettings::default();
        settings.wtb.volume_threshold = 15;
        settings.wtb.profit_threshold = 10;
        settings.wtb.avg_price_cap = 600;
        let ids = |items: Vec<ItemPriceInfo>| items.into_iter().map(|i| i.wfm_id).collect::<Vec<_>>();
        assert_eq!(ids(get_interesting_items(&settings, &prices)), vec!["d", "a"]);
        settings.wtb.profit_threshold = -1;
        assert_eq!(ids(get_interesting_items(&settings, &prices)), vec!["d", "b", "a"]);
    }

    #[test]
    fn the_shift_filter_applies_only_to_items_that_have_a_shift() {
        use crate::trader::blend::Effective;
        let eff = |id: &str, shift: Option<f64>| Effective {
            stats: stats(id, "", 50.0, 50.0, 100.0),
            week_price_shift: shift,
            guarded: id == "falling",
            closed: shift.is_some(),
            inferred: true,
        };
        let prices = StatsPriceSource::from_effective(vec![eff("rising", Some(4.0)), eff("falling", Some(-9.0)), eff("unknown", None)], |id| Some(format!("{id}_slug")));
        let mut settings = ItemSettings::default();
        settings.wtb.price_shift_threshold = -5;
        let mut ids: Vec<String> = get_interesting_items(&settings, &prices).into_iter().map(|i| i.wfm_id).collect();
        ids.sort();
        assert_eq!(ids, vec!["rising", "unknown"], "-9 is below the -5 threshold; no shift always passes");
        settings.wtb.price_shift_threshold = -1;
        assert_eq!(get_interesting_items(&settings, &prices).len(), 3, "disabled");
        assert!(prices.find_by("falling", &None).unwrap().guarded);
        assert_eq!(prices.find_by("rising", &None).unwrap().week_price_shift, 4.0);
    }

    #[test]
    fn the_trading_tax_cap_drops_expensive_to_trade_items() {
        let prices = source(vec![stats("cheap", "", 50.0, 50.0, 100.0), stats("dear", "", 50.0, 50.0, 100.0)]).with_trade_tax(|id| Some(if id == "dear" { 1_000_000 } else { 2_000 }));
        let mut settings = ItemSettings::default();
        assert_eq!(get_interesting_items(&settings, &prices).len(), 2, "the cap is disabled by default");
        settings.wtb.trading_tax_cap = 500_000;
        assert_eq!(get_interesting_items(&settings, &prices).into_iter().map(|i| i.wfm_id).collect::<Vec<_>>(), vec!["cheap"]);
    }

    #[test]
    fn only_the_maximum_rank_of_a_ranked_item_is_a_buy_candidate() {
        let prices = source(vec![
            stats("mod", "rank=0", 90.0, 50.0, 20.0),
            stats("mod", "rank=10", 40.0, 50.0, 80.0),
            stats("set", "", 30.0, 50.0, 100.0),
            stats("relic", "subtype=intact", 25.0, 50.0, 10.0),
            stats("unknown_max", "rank=0", 20.0, 50.0, 10.0),
        ])
        .with_max_rank(|id| (id == "mod").then_some(10));
        let ids: Vec<String> = get_interesting_items(&ItemSettings::default(), &prices).into_iter().map(|i| i.uuid).collect();
        assert_eq!(ids, vec!["mod:rank=10", "set:", "relic:subtype=intact", "unknown_max:rank=0"], "rank 0 of a rank-10 mod is dropped; unranked items and items with no known max rank stay");
        assert_eq!(prices.find_by("mod", &sub_type_from_key("rank=0")).unwrap().max_rank, Some(10), "find_by still serves the rank-0 key for stock and wish-list items");
    }

    #[test]
    fn buy_candidates_are_capped_and_need_buy_mode() {
        let rows = (0..200).map(|n| stats(&format!("i{n:03}"), "", 100.0 + n as f64, 50.0, 10.0)).collect();
        let prices = source(rows);
        let mut settings = Settings::default();
        assert_eq!(buy_candidate_ids(&settings, &prices).len(), MAX_BUY_CANDIDATES);
        settings.live_scraper.general.trade_modes.retain(|m| *m != TradeMode::Buy);
        assert!(buy_candidate_ids(&settings, &prices).is_empty());
    }
}
