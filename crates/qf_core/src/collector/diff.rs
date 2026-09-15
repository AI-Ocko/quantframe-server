use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;

use super::orders::V2Order;

pub const TOP_ORDERS: usize = 5;

/// A row of `last_seen_orders`, without `item_id` (a sweep covers one item).
#[derive(Debug, Clone, PartialEq)]
pub struct SeenOrder {
    pub order_id: String,
    pub sub_type: String,
    pub side: String,
    pub platinum: i64,
    pub quantity: i64,
    pub user_id: String,
    pub first_seen: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SweepSummary {
    pub sub_type: String,
    pub min_sell: Option<i64>,
    pub max_buy: Option<i64>,
    pub sell_count: i64,
    pub buy_count: i64,
    pub sell_ingame: i64,
    pub buy_ingame: i64,
    pub top_sells: Vec<[i64; 2]>,
    pub top_buys: Vec<[i64; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Changed {
    pub before: SeenOrder,
    pub after: SeenOrder,
}

impl Changed {
    pub fn quantity_drop(&self) -> i64 {
        (self.before.quantity - self.after.quantity).max(0)
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct OrderDiff {
    pub new: Vec<SeenOrder>,
    pub changed: Vec<Changed>,
    pub vanished: Vec<SeenOrder>,
}

/// Visible orders as `last_seen_orders` rows. `first_seen` is the sweep time.
pub fn to_seen(orders: &[V2Order], swept_at: &str) -> Vec<SeenOrder> {
    orders
        .iter()
        .filter(|o| o.visible)
        .map(|o| SeenOrder {
            order_id: o.id.clone(),
            sub_type: o.sub_type_key(),
            side: o.side.clone(),
            platinum: o.platinum,
            quantity: o.quantity,
            user_id: o.user.id.clone(),
            first_seen: swept_at.to_string(),
        })
        .collect()
}

/// One summary per sub-type (amendment B6).
pub fn summarize(orders: &[V2Order]) -> Vec<SweepSummary> {
    let mut groups: BTreeMap<String, Vec<&V2Order>> = BTreeMap::new();
    for order in orders.iter().filter(|o| o.visible) {
        groups.entry(order.sub_type_key()).or_default().push(order);
    }
    groups
        .into_iter()
        .map(|(sub_type, group)| {
            let sells: Vec<&V2Order> = group.iter().copied().filter(|o| o.side == "sell").collect();
            let buys: Vec<&V2Order> = group.iter().copied().filter(|o| o.side == "buy").collect();
            let mut ingame_sells: Vec<&V2Order> = sells.iter().copied().filter(|o| o.is_ingame()).collect();
            let mut ingame_buys: Vec<&V2Order> = buys.iter().copied().filter(|o| o.is_ingame()).collect();
            ingame_sells.sort_by_key(|o| o.platinum);
            ingame_buys.sort_by_key(|o| std::cmp::Reverse(o.platinum));
            SweepSummary {
                sub_type,
                min_sell: ingame_sells.first().map(|o| o.platinum),
                max_buy: ingame_buys.first().map(|o| o.platinum),
                sell_count: sells.len() as i64,
                buy_count: buys.len() as i64,
                sell_ingame: ingame_sells.len() as i64,
                buy_ingame: ingame_buys.len() as i64,
                top_sells: ingame_sells.iter().take(TOP_ORDERS).map(|o| [o.platinum, o.quantity]).collect(),
                top_buys: ingame_buys.iter().take(TOP_ORDERS).map(|o| [o.platinum, o.quantity]).collect(),
            }
        })
        .collect()
}

/// Compares the previous live set with this sweep. A price or quantity edit keeps the
/// order id, so it counts as a change, not a vanish.
pub fn diff_orders(previous: &[SeenOrder], current: &[SeenOrder]) -> OrderDiff {
    let before: HashMap<&str, &SeenOrder> = previous.iter().map(|o| (o.order_id.as_str(), o)).collect();
    let current_ids: HashSet<&str> = current.iter().map(|o| o.order_id.as_str()).collect();
    let mut diff = OrderDiff::default();
    for order in current {
        match before.get(order.order_id.as_str()) {
            None => diff.new.push(order.clone()),
            Some(prev) if prev.platinum != order.platinum || prev.quantity != order.quantity => {
                diff.changed.push(Changed {
                    before: (*prev).clone(),
                    after: SeenOrder { first_seen: prev.first_seen.clone(), ..order.clone() },
                });
            }
            Some(_) => {}
        }
    }
    for prev in previous {
        if !current_ids.contains(prev.order_id.as_str()) {
            diff.vanished.push(prev.clone());
        }
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::orders::parse_orders_response;

    const SMALL: &str = include_str!("../../tests/fixtures/orders_small.json");
    const ARCANE: &str = include_str!("../../tests/fixtures/orders_arcane_energize.json");

    fn seen(id: &str, platinum: i64, quantity: i64, first_seen: &str) -> SeenOrder {
        SeenOrder {
            order_id: id.to_string(),
            sub_type: "rank=0".to_string(),
            side: "sell".to_string(),
            platinum,
            quantity,
            user_id: "u1".to_string(),
            first_seen: first_seen.to_string(),
        }
    }

    #[test]
    fn summaries_split_by_rank_and_use_ingame_prices() {
        let orders = parse_orders_response(SMALL).unwrap();
        let summaries = summarize(&orders);
        assert_eq!(
            summaries,
            vec![
                SweepSummary {
                    sub_type: "rank=0".into(),
                    min_sell: Some(20),
                    max_buy: Some(15),
                    sell_count: 3,
                    buy_count: 2,
                    sell_ingame: 2,
                    buy_ingame: 1,
                    top_sells: vec![[20, 2], [25, 1]],
                    top_buys: vec![[15, 5]],
                },
                SweepSummary {
                    sub_type: "rank=5".into(),
                    min_sell: Some(90),
                    max_buy: None,
                    sell_count: 1,
                    buy_count: 0,
                    sell_ingame: 1,
                    buy_ingame: 0,
                    top_sells: vec![[90, 1]],
                    top_buys: vec![],
                },
            ]
        );
    }

    #[test]
    fn summary_counts_cover_every_visible_recorded_order() {
        let orders = parse_orders_response(ARCANE).unwrap();
        let visible = orders.iter().filter(|o| o.visible).count() as i64;
        let counted: i64 = summarize(&orders).iter().map(|s| s.sell_count + s.buy_count).sum();
        assert_eq!(counted, visible);
        assert!(summarize(&orders).iter().all(|s| s.top_sells.len() <= TOP_ORDERS));
    }

    #[test]
    fn to_seen_skips_hidden_orders_and_stamps_first_seen() {
        let orders = parse_orders_response(SMALL).unwrap();
        let rows = to_seen(&orders, "2026-09-15T00:00:00Z");
        assert_eq!(rows.len(), 6);
        assert!(rows.iter().all(|r| r.order_id != "h1"));
        assert!(rows.iter().all(|r| r.first_seen == "2026-09-15T00:00:00Z"));
        assert_eq!(rows[3].sub_type, "rank=5");
    }

    #[test]
    fn diff_finds_new_changed_and_vanished_orders() {
        let previous = vec![
            seen("keep", 10, 1, "t0"),
            seen("edit", 10, 3, "t0"),
            seen("gone", 12, 1, "t0"),
        ];
        let current = vec![
            seen("keep", 10, 1, "t1"),
            seen("edit", 11, 1, "t1"),
            seen("fresh", 9, 1, "t1"),
        ];
        let diff = diff_orders(&previous, &current);
        assert_eq!(diff.new, vec![seen("fresh", 9, 1, "t1")]);
        assert_eq!(diff.vanished, vec![seen("gone", 12, 1, "t0")]);
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].before, seen("edit", 10, 3, "t0"));
        assert_eq!(diff.changed[0].after, seen("edit", 11, 1, "t0"), "edits keep first_seen");
        assert_eq!(diff.changed[0].quantity_drop(), 2);
    }

    #[test]
    fn quantity_increase_is_not_a_drop() {
        let change = Changed { before: seen("a", 10, 1, "t0"), after: seen("a", 10, 4, "t0") };
        assert_eq!(change.quantity_drop(), 0);
    }
}
