//! Inferred and closed statistics merged into the trader's effective inputs (spec §25 P4–P5). Pure, no I/O.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::collector::closed::ClosedStats;
use crate::collector::stats::ItemStats;
use crate::collector::ts;
use crate::enums::PriceSourceMode;
use crate::trader::price_source::is_disabled;

pub const GUARD_MIN_WEEK_TRADES: f64 = 20.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Effective {
    pub stats: ItemStats,
    pub week_price_shift: Option<f64>,
    pub guarded: bool,
    /// The price fields come from closed statistics.
    pub closed: bool,
    /// The key has an inferred row, i.e. the collector knows it (spec §25 P12).
    pub inferred: bool,
}

/// `closed` must already be limited to fresh items (`closed::load_fresh`).
pub fn blend(inferred: Vec<ItemStats>, closed: Vec<ClosedStats>, mode: PriceSourceMode, guard_pct: i64, now: DateTime<Utc>) -> Vec<Effective> {
    let plain = |stats: ItemStats| Effective { stats, week_price_shift: None, guarded: false, closed: false, inferred: true };
    if mode == PriceSourceMode::Inferred {
        return inferred.into_iter().map(plain).collect();
    }
    let mut closed_by_key: BTreeMap<(String, String), ClosedStats> = closed.into_iter().map(|c| ((c.item_id.clone(), c.sub_type.clone()), c)).collect();
    let mut out: Vec<Effective> = inferred
        .into_iter()
        .map(|i| match closed_by_key.remove(&(i.item_id.clone(), i.sub_type.clone())) {
            Some(c) => merge(Some(i), c, guard_pct, now),
            None => plain(i),
        })
        .collect();
    out.extend(closed_by_key.into_values().map(|c| merge(None, c, guard_pct, now)));
    out
}

fn merge(inferred: Option<ItemStats>, c: ClosedStats, guard_pct: i64, now: DateTime<Utc>) -> Effective {
    let known = inferred.is_some();
    // spec §25 P5: the collector sees a falling market within minutes; the closed average is a week old by construction.
    let fast_drop = inferred.as_ref().and_then(|i| {
        let (recent, closed_avg) = (i.avg_price?, c.moving_avg?);
        (!is_disabled(guard_pct) && i.volume * 7.0 >= GUARD_MIN_WEEK_TRADES && recent < closed_avg * (1.0 - guard_pct as f64 / 100.0)).then_some(recent)
    });
    Effective {
        stats: ItemStats {
            item_id: c.item_id,
            sub_type: c.sub_type,
            volume: c.volume,
            avg_price: c.avg_price,
            moving_avg: fast_drop.or(c.moving_avg),
            profit: inferred.as_ref().and_then(|i| i.profit),
            min_price: c.min_price,
            max_price: c.max_price,
            median: c.median,
            history_days: inferred.as_ref().map(|i| i.history_days).unwrap_or(0),
            // spec §25 P12: `warm` is the live/dry-run gate, and a key the collector has never seen must not open it.
            warm: c.warm && known,
            updated_at: inferred.map(|i| i.updated_at).unwrap_or_else(|| ts(now)),
        },
        week_price_shift: c.week_price_shift,
        guarded: fast_drop.is_some(),
        closed: true,
        inferred: known,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;

    fn now() -> DateTime<Utc> {
        parse_ts("2026-09-20T08:00:00Z").unwrap()
    }
    fn inferred(id: &str, volume: f64, moving_avg: f64, avg_48h: f64, profit: f64) -> ItemStats {
        ItemStats { item_id: id.into(), sub_type: String::new(), volume, avg_price: Some(avg_48h), moving_avg: Some(moving_avg), profit: Some(profit), min_price: Some(1), max_price: Some(2), median: Some(moving_avg), history_days: 5, warm: false, updated_at: "2026-09-20T07:55:00Z".into() }
    }
    fn closed(id: &str, volume: f64, moving_avg: f64) -> ClosedStats {
        ClosedStats { item_id: id.into(), sub_type: String::new(), volume, moving_avg: Some(moving_avg), median: Some(moving_avg + 1.0), avg_price: Some(moving_avg + 2.0), min_price: Some(40), max_price: Some(90), week_price_shift: Some(-3.0), days: 7, trades: (volume * 7.0) as i64, warm: true }
    }

    #[test]
    fn inferred_mode_is_the_identity() {
        let rows = vec![inferred("a", 9.0, 70.0, 50.0, 12.0)];
        let out = blend(rows.clone(), vec![closed("a", 30.0, 66.0)], PriceSourceMode::Inferred, 10, now());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], Effective { stats: rows[0].clone(), week_price_shift: None, guarded: false, closed: false, inferred: true });
    }

    #[test]
    fn closed_mode_takes_the_closed_fields_and_keeps_the_inferred_profit() {
        let out = blend(vec![inferred("a", 9.0, 70.0, 69.0, 12.0)], vec![closed("a", 30.0, 66.0)], PriceSourceMode::Closed, 10, now());
        let e = &out[0];
        assert!(e.closed && !e.guarded && e.inferred);
        assert_eq!((e.stats.volume, e.stats.moving_avg, e.stats.median, e.stats.avg_price), (30.0, Some(66.0), Some(67.0), Some(68.0)));
        assert_eq!((e.stats.min_price, e.stats.max_price, e.stats.warm), (Some(40), Some(90), true));
        assert_eq!((e.stats.profit, e.stats.history_days), (Some(12.0), 5), "profit and history stay inferred");
        assert_eq!(e.week_price_shift, Some(-3.0));
    }

    #[test]
    fn closed_mode_falls_back_per_key_and_keeps_closed_only_keys() {
        let out = blend(vec![inferred("only_inferred", 9.0, 70.0, 69.0, 12.0)], vec![closed("only_closed", 30.0, 66.0)], PriceSourceMode::Closed, 10, now());
        assert_eq!(out.len(), 2);
        let c = out.iter().find(|e| e.stats.item_id == "only_closed").unwrap();
        assert_eq!((c.stats.profit, c.stats.history_days, c.closed), (None, 0, true));
        assert_eq!(c.stats.updated_at, ts(now()));
        assert!(!c.inferred, "the collector does not know this key");
        assert!(!c.stats.warm, "a closed-only key is never warm, so it keeps routing to dry-run as on main");
        let i = out.iter().find(|e| e.stats.item_id == "only_inferred").unwrap();
        assert_eq!((i.stats.moving_avg, i.closed, i.week_price_shift), (Some(70.0), false, None));
        assert!(i.inferred);
    }

    #[test]
    fn the_guard_lowers_closed_avg_only_past_the_percentage_with_enough_trades_and_when_enabled() {
        let run = |volume: f64, avg_48h: f64, pct: i64| blend(vec![inferred("a", volume, 99.0, avg_48h, 5.0)], vec![closed("a", 30.0, 100.0)], PriceSourceMode::Closed, pct, now()).remove(0);
        let fired = run(3.0, 85.0, 10);
        assert!(fired.guarded);
        assert_eq!(fired.stats.moving_avg, Some(85.0));
        assert!(!run(3.0, 95.0, 10).guarded, "5 % below is inside the 10 % band");
        assert!(!run(3.0, 90.0, 10).guarded, "exactly 10 % below does not fire");
        assert!(!run(2.0, 85.0, 10).guarded, "14 inferred trades a week is too thin");
        assert!(!run(3.0, 85.0, -1).guarded, "disabled");
        assert!(!run(3.0, 120.0, 10).guarded, "the guard never raises closed_avg");
    }
}
