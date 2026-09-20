//! Both price bases side by side, with buy-candidate membership under each (spec §25 P8). Pure, no I/O.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::app::ItemSettings;
use crate::collector::closed::ClosedStats;
use crate::collector::stats::ItemStats;
use crate::enums::PriceSourceMode;
use crate::trader::blend::blend;
use crate::trader::price_source::{get_interesting_items, StatsPriceSource};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PriceSourceRow {
    pub item_id: String,
    pub sub_type: String,
    pub name: String,
    pub wfm_url: String,
    pub inferred_volume: Option<f64>,
    pub inferred_moving_avg: Option<f64>,
    pub closed_volume: Option<f64>,
    pub closed_moving_avg: Option<f64>,
    pub closed_days: Option<usize>,
    pub week_price_shift: Option<f64>,
    pub profit: Option<f64>,
    pub warm_inferred: bool,
    pub warm_closed: bool,
    pub candidate_inferred: bool,
    pub candidate_closed: bool,
    pub guarded: bool,
    pub fetched_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CandidateCounts {
    pub inferred: usize,
    pub closed: usize,
    pub both: usize,
}

pub struct ItemLookup {
    pub name: String,
    pub wfm_url: String,
    pub trade_tax: i64,
}

/// `get_interesting_items` caps at 150, so each count is at most the number the trader would actually work.
pub fn compare(
    inferred: Vec<ItemStats>,
    closed: Vec<ClosedStats>,
    settings: &ItemSettings,
    guard_pct: i64,
    now: DateTime<Utc>,
    lookup: impl Fn(&str) -> Option<ItemLookup>,
    fetched_at: &HashMap<String, String>,
) -> (Vec<PriceSourceRow>, CandidateCounts) {
    let candidates = |mode: PriceSourceMode| -> HashSet<String> {
        let source = StatsPriceSource::from_effective(blend(inferred.clone(), closed.clone(), mode, guard_pct, now), |id| lookup(id).map(|l| l.wfm_url))
            .with_trade_tax(|id| lookup(id).map(|l| l.trade_tax));
        get_interesting_items(settings, &source).into_iter().map(|i| i.uuid).collect()
    };
    let (as_inferred, as_closed) = (candidates(PriceSourceMode::Inferred), candidates(PriceSourceMode::Closed));
    let guarded: HashSet<(String, String)> = blend(inferred.clone(), closed.clone(), PriceSourceMode::Closed, guard_pct, now)
        .into_iter()
        .filter(|e| e.guarded)
        .map(|e| (e.stats.item_id, e.stats.sub_type))
        .collect();

    let mut keys: BTreeMap<(String, String), (Option<ItemStats>, Option<ClosedStats>)> = BTreeMap::new();
    for i in inferred {
        let key = (i.item_id.clone(), i.sub_type.clone());
        keys.entry(key).or_default().0 = Some(i);
    }
    for c in closed {
        let key = (c.item_id.clone(), c.sub_type.clone());
        keys.entry(key).or_default().1 = Some(c);
    }
    let rows: Vec<PriceSourceRow> = keys
        .into_iter()
        .filter_map(|((item_id, sub_type), (i, c))| {
            let item = lookup(&item_id)?;
            let uuid = format!("{item_id}:{sub_type}");
            Some(PriceSourceRow {
                name: item.name,
                wfm_url: item.wfm_url,
                inferred_volume: i.as_ref().map(|i| i.volume),
                inferred_moving_avg: i.as_ref().and_then(|i| i.moving_avg),
                closed_volume: c.as_ref().map(|c| c.volume),
                closed_moving_avg: c.as_ref().and_then(|c| c.moving_avg),
                closed_days: c.as_ref().map(|c| c.days),
                week_price_shift: c.as_ref().and_then(|c| c.week_price_shift),
                profit: i.as_ref().and_then(|i| i.profit),
                warm_inferred: i.as_ref().is_some_and(|i| i.warm),
                warm_closed: c.as_ref().is_some_and(|c| c.warm),
                candidate_inferred: as_inferred.contains(&uuid),
                candidate_closed: as_closed.contains(&uuid),
                guarded: guarded.contains(&(item_id.clone(), sub_type.clone())),
                fetched_at: fetched_at.get(&item_id).cloned(),
                item_id,
                sub_type,
            })
        })
        .collect();
    let counts = CandidateCounts {
        inferred: rows.iter().filter(|r| r.candidate_inferred).count(),
        closed: rows.iter().filter(|r| r.candidate_closed).count(),
        both: rows.iter().filter(|r| r.candidate_inferred && r.candidate_closed).count(),
    };
    (rows, counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;

    fn inferred(id: &str, volume: f64, moving_avg: f64) -> ItemStats {
        ItemStats { item_id: id.into(), sub_type: String::new(), volume, avg_price: Some(moving_avg), moving_avg: Some(moving_avg), profit: Some(20.0), min_price: Some(1), max_price: Some(2), median: Some(moving_avg), history_days: 5, warm: false, updated_at: "2026-09-20T07:55:00Z".into() }
    }
    fn closed(id: &str, volume: f64, moving_avg: f64) -> ClosedStats {
        ClosedStats { item_id: id.into(), sub_type: String::new(), volume, moving_avg: Some(moving_avg), median: Some(moving_avg), avg_price: Some(moving_avg), min_price: Some(1), max_price: Some(2), week_price_shift: Some(1.0), days: 7, trades: (volume * 7.0) as i64, warm: true }
    }

    #[test]
    fn rows_carry_both_bases_and_candidate_membership_under_each_mode() {
        let mut settings = ItemSettings::default(); // volume_threshold 15, profit_threshold 10, avg_price_cap 600
        settings.wtb.volume_threshold = 15;
        let (rows, counts) = compare(
            vec![inferred("undercounted", 9.0, 70.0), inferred("busy", 40.0, 50.0), inferred("untradable", 99.0, 5.0)],
            vec![closed("undercounted", 30.0, 66.0), closed("busy", 60.0, 48.0), closed("closed_only", 25.0, 10.0)],
            &settings,
            10,
            parse_ts("2026-09-20T08:00:00Z").unwrap(),
            |id| (id != "untradable").then(|| ItemLookup { name: format!("Name {id}"), wfm_url: format!("{id}_slug"), trade_tax: 0 }),
            &HashMap::from([("busy".to_string(), "2026-09-20T01:00:00Z".to_string())]),
        );
        assert_eq!(rows.len(), 3, "untradable ids are dropped");
        let row = |id: &str| rows.iter().find(|r| r.item_id == id).unwrap();
        let u = row("undercounted");
        assert_eq!((u.inferred_volume, u.closed_volume, u.candidate_inferred, u.candidate_closed), (Some(9.0), Some(30.0), false, true));
        assert_eq!((u.warm_inferred, u.warm_closed, u.closed_days, u.name.as_str()), (false, true, Some(7), "Name undercounted"));
        assert_eq!((row("busy").candidate_inferred, row("busy").candidate_closed), (true, true));
        assert_eq!(row("busy").fetched_at.as_deref(), Some("2026-09-20T01:00:00Z"));
        let c = row("closed_only");
        assert_eq!((c.inferred_volume, c.profit, c.candidate_closed), (None, None, false), "no inferred profit, so the profit filter rejects it");
        assert_eq!(counts, CandidateCounts { inferred: 1, closed: 2, both: 1 });
    }
}
