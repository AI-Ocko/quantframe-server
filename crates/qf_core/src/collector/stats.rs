//! Pure probable-trade statistics (spec §5.5, amendments B8–B9). No I/O, so unit tests cover every rule.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;

use super::ts;

#[derive(Debug, Clone)]
pub struct StatsConfig {
    pub relist_window: Duration,
    pub bulk_pull_threshold: i64,
    pub bulk_window: Duration,
    pub gap_factor: f64,
    pub window: Duration,
    pub avg_window: Duration,
    pub warm_days: i64,
    pub warm_min_trades: usize,
    pub side_min_trades: usize,
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            relist_window: Duration::hours(2),
            bulk_pull_threshold: 3,
            bulk_window: Duration::minutes(30),
            gap_factor: 3.0,
            window: Duration::days(7),
            avg_window: Duration::hours(48),
            warm_days: 7,
            warm_min_trades: 10,
            side_min_trades: 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Trade,
    Relist,
    Bulk,
}

impl Resolution {
    pub fn as_str(self) -> &'static str {
        match self {
            Resolution::Trade => "trade",
            Resolution::Relist => "relist",
            Resolution::Bulk => "bulk",
        }
    }
}

/// Decides a pending full vanish once its relist window has closed. The gap rule was applied when the vanish was recorded.
pub fn resolve_vanish(user_vanishes_in_bulk_window: i64, user_relisted: bool, cfg: &StatsConfig) -> Resolution {
    if user_vanishes_in_bulk_window >= cfg.bulk_pull_threshold {
        Resolution::Bulk
    } else if user_relisted {
        Resolution::Relist
    } else {
        Resolution::Trade
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trade {
    /// Side of the vanished order: a vanished `sell` is someone buying.
    pub side: String,
    pub platinum: i64,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiveSpread {
    pub min_sell: Option<i64>,
    pub max_buy: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ItemStats {
    pub item_id: String,
    pub sub_type: String,
    pub volume: f64,
    pub avg_price: Option<f64>,
    pub moving_avg: Option<f64>,
    pub profit: Option<f64>,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
    pub median: Option<f64>,
    pub history_days: i64,
    pub warm: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DailyStat {
    pub day: NaiveDate,
    pub volume: i64,
    pub median: Option<f64>,
    pub min_price: Option<i64>,
    pub max_price: Option<i64>,
}

pub fn median(values: &[i64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    Some(if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) as f64 / 2.0
    } else {
        sorted[mid] as f64
    })
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

pub fn compute_item_stats(
    item_id: &str,
    sub_type: &str,
    trades: &[Trade],
    spread: &LiveSpread,
    first_swept_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    cfg: &StatsConfig,
) -> ItemStats {
    let week: Vec<&Trade> = trades.iter().filter(|t| t.at > now - cfg.window && t.at <= now).collect();
    let prices: Vec<i64> = week.iter().map(|t| t.platinum).collect();
    let recent: Vec<f64> = week
        .iter()
        .filter(|t| t.at > now - cfg.avg_window)
        .map(|t| t.platinum as f64)
        .collect();

    let mut by_day: BTreeMap<NaiveDate, Vec<i64>> = BTreeMap::new();
    for t in &week {
        by_day.entry(t.at.date_naive()).or_default().push(t.platinum);
    }
    let daily_medians: Vec<f64> = by_day.values().filter_map(|v| median(v)).collect();

    let sells: Vec<i64> = week.iter().filter(|t| t.side == "sell").map(|t| t.platinum).collect();
    let buys: Vec<i64> = week.iter().filter(|t| t.side == "buy").map(|t| t.platinum).collect();
    let profit = match (median(&sells), median(&buys)) {
        (Some(sell), Some(buy)) if sells.len() >= cfg.side_min_trades && buys.len() >= cfg.side_min_trades => {
            Some(sell - buy)
        }
        _ => match (spread.min_sell, spread.max_buy) {
            (Some(sell), Some(buy)) => Some((sell - buy) as f64),
            _ => None,
        },
    };

    let history_days = first_swept_at.map(|f| (now - f).num_days().max(0)).unwrap_or(0);
    ItemStats {
        item_id: item_id.to_string(),
        sub_type: sub_type.to_string(),
        volume: week.len() as f64 / cfg.window.num_days() as f64,
        avg_price: mean(&recent),
        moving_avg: mean(&daily_medians),
        profit,
        min_price: prices.iter().copied().min(),
        max_price: prices.iter().copied().max(),
        median: median(&prices),
        history_days,
        warm: history_days >= cfg.warm_days && week.len() >= cfg.warm_min_trades,
        updated_at: ts(now),
    }
}

pub fn daily_rollup(trades: &[Trade]) -> Vec<DailyStat> {
    let mut by_day: BTreeMap<NaiveDate, Vec<i64>> = BTreeMap::new();
    for t in trades {
        by_day.entry(t.at.date_naive()).or_default().push(t.platinum);
    }
    by_day
        .into_iter()
        .map(|(day, prices)| DailyStat {
            day,
            volume: prices.len() as i64,
            median: median(&prices),
            min_price: prices.iter().copied().min(),
            max_price: prices.iter().copied().max(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;

    fn now() -> DateTime<Utc> {
        parse_ts("2026-09-15T12:00:00Z").unwrap()
    }

    fn trade(side: &str, platinum: i64, hours_ago: i64) -> Trade {
        Trade { side: side.into(), platinum, at: now() - Duration::hours(hours_ago) }
    }

    /// Two trades on each of the last 7 days: a sell at 100+d and a buy at 110+d.
    fn week() -> Vec<Trade> {
        (0..7)
            .flat_map(|d| [trade("sell", 100 + d, d * 24 + 1), trade("buy", 110 + d, d * 24 + 2)])
            .collect()
    }

    #[test]
    fn median_handles_odd_even_and_empty() {
        assert_eq!(median(&[3, 1, 2]), Some(2.0));
        assert_eq!(median(&[4, 1, 2, 3]), Some(2.5));
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn vanish_resolution_prefers_bulk_then_relist() {
        let cfg = StatsConfig::default();
        assert_eq!(resolve_vanish(3, true, &cfg), Resolution::Bulk);
        assert_eq!(resolve_vanish(2, true, &cfg), Resolution::Relist);
        assert_eq!(resolve_vanish(1, false, &cfg), Resolution::Trade);
        assert_eq!(Resolution::Relist.as_str(), "relist");
    }

    #[test]
    fn a_normal_week_of_trades() {
        let cfg = StatsConfig::default();
        let first = Some(now() - Duration::days(10));
        let stats = compute_item_stats("item1", "rank=0", &week(), &LiveSpread::default(), first, now(), &cfg);
        assert_eq!(stats.volume, 2.0);
        assert_eq!(stats.avg_price, Some(105.5));
        assert_eq!(stats.moving_avg, Some(108.0));
        assert_eq!(stats.min_price, Some(100));
        assert_eq!(stats.max_price, Some(116));
        assert_eq!(stats.median, Some(108.0));
        assert_eq!(stats.profit, Some(-10.0));
        assert_eq!(stats.history_days, 10);
        assert!(stats.warm);
        assert_eq!(stats.updated_at, "2026-09-15T12:00:00Z");
    }

    #[test]
    fn trades_older_than_seven_days_are_ignored() {
        let cfg = StatsConfig::default();
        let mut trades = week();
        trades.push(trade("sell", 1, 8 * 24));
        let stats = compute_item_stats("item1", "", &trades, &LiveSpread::default(), None, now(), &cfg);
        assert_eq!(stats.volume, 2.0);
        assert_eq!(stats.min_price, Some(100));
        assert_eq!(stats.history_days, 0);
        assert!(!stats.warm);
    }

    #[test]
    fn warm_needs_seven_days_and_ten_trades() {
        let cfg = StatsConfig::default();
        let ten: Vec<Trade> = (0..10).map(|h| trade("sell", 50, h)).collect();
        let nine = &ten[..9];
        let seven_days = Some(now() - Duration::days(7));
        let six_days = Some(now() - Duration::days(7) + Duration::seconds(1));
        let spread = LiveSpread::default();
        assert!(compute_item_stats("i", "", &ten, &spread, seven_days, now(), &cfg).warm);
        assert!(!compute_item_stats("i", "", &ten, &spread, six_days, now(), &cfg).warm);
        assert!(!compute_item_stats("i", "", nine, &spread, seven_days, now(), &cfg).warm);
    }

    #[test]
    fn profit_uses_trade_medians_when_both_sides_have_three() {
        let cfg = StatsConfig::default();
        let trades = vec![
            trade("sell", 100, 1),
            trade("sell", 110, 2),
            trade("sell", 120, 3),
            trade("buy", 80, 1),
            trade("buy", 90, 2),
            trade("buy", 95, 3),
        ];
        let spread = LiveSpread { min_sell: Some(500), max_buy: Some(1) };
        assert_eq!(compute_item_stats("i", "", &trades, &spread, None, now(), &cfg).profit, Some(20.0));
    }

    #[test]
    fn profit_falls_back_to_the_live_spread_when_data_is_thin() {
        let cfg = StatsConfig::default();
        let trades = vec![trade("sell", 100, 1), trade("sell", 110, 2), trade("sell", 120, 3), trade("buy", 80, 1)];
        let spread = LiveSpread { min_sell: Some(105), max_buy: Some(85) };
        assert_eq!(compute_item_stats("i", "", &trades, &spread, None, now(), &cfg).profit, Some(20.0));
        let one_sided = LiveSpread { min_sell: Some(105), max_buy: None };
        assert_eq!(compute_item_stats("i", "", &trades, &one_sided, None, now(), &cfg).profit, None);
    }

    #[test]
    fn daily_rollup_groups_by_utc_day() {
        let trades = vec![trade("sell", 10, 1), trade("buy", 30, 2), trade("sell", 20, 13)];
        let days = daily_rollup(&trades);
        assert_eq!(
            days,
            vec![
                DailyStat { day: "2026-09-14".parse().unwrap(), volume: 1, median: Some(20.0), min_price: Some(20), max_price: Some(20) },
                DailyStat { day: "2026-09-15".parse().unwrap(), volume: 2, median: Some(20.0), min_price: Some(10), max_price: Some(30) },
            ]
        );
    }
}
