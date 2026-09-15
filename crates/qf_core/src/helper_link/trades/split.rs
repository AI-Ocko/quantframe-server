//! Platinum split across the items of one trade (amendment E8).

use std::collections::HashMap;

use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::Error;
use wf_market::enums::OrderType;

use super::{Direction, ResolvedItem};
use crate::collector::{db_err, stmt};
use crate::trader::price_source::key_of;

/// Whole-platinum shares proportional to `weights`. An unknown or zero weight takes the mean of
/// the known ones, so it is neither starved nor favoured; all unknown means equal shares.
/// The rounding remainder lands on the first item.
pub fn split_platinum(total: i64, weights: &[Option<f64>]) -> Vec<i64> {
    if weights.is_empty() {
        return Vec::new();
    }
    let known: Vec<f64> = weights.iter().flatten().copied().filter(|w| *w > 0.0).collect();
    let fill = if known.is_empty() { 1.0 } else { known.iter().sum::<f64>() / known.len() as f64 };
    let filled: Vec<f64> = weights.iter().map(|w| match w { Some(v) if *v > 0.0 => *v, _ => fill }).collect();
    let sum: f64 = filled.iter().sum();
    let mut shares: Vec<i64> = filled.iter().map(|w| (total as f64 * w / sum).round() as i64).collect();
    let remainder = total - shares.iter().sum::<i64>();
    shares[0] += remainder;
    shares
}

/// Line weight = unit price × quantity. The unit price is the user's own order for that item and
/// sub type (sell order for a sale, buy order for a purchase), else the collector median.
pub fn weights_for(
    items: &[ResolvedItem],
    direction: Direction,
    own_price: &dyn Fn(&ResolvedItem, OrderType) -> Option<i64>,
    median: &dyn Fn(&ResolvedItem) -> Option<f64>,
) -> Vec<Option<f64>> {
    let order_type = match direction {
        Direction::Sale => OrderType::Sell,
        Direction::Purchase => OrderType::Buy,
    };
    items
        .iter()
        .map(|item| own_price(item, order_type).map(|p| p as f64).or_else(|| median(item)).map(|unit| unit * item.quantity as f64))
        .collect()
}

pub fn price_items(items: &mut [ResolvedItem], total: i64, weights: &[Option<f64>]) {
    for (item, price) in items.iter_mut().zip(split_platinum(total, weights)) {
        item.price = price;
    }
}

/// `item_stats.median` keyed by `(wfm_id, sub-type key)` for the given items.
pub async fn medians(conn: &DatabaseConnection, items: &[ResolvedItem]) -> Result<HashMap<(String, String), f64>, Error> {
    const C: &str = "HelperLink:Medians";
    let mut out = HashMap::new();
    for item in items {
        let key = key_of(&item.sub_type);
        let row = conn
            .query_one(stmt(
                "SELECT median FROM item_stats WHERE item_id = ? AND sub_type = ?",
                vec![item.wfm_id.clone().into(), key.clone().into()],
            ))
            .await
            .map_err(|e| db_err(C, e))?;
        if let Some(median) = row.and_then(|r| r.try_get::<Option<f64>>("", "median").ok().flatten()) {
            out.insert((item.wfm_id.clone(), key), median);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(slug: &str, quantity: i64) -> ResolvedItem {
        ResolvedItem {
            name: slug.into(),
            slug: slug.into(),
            wfm_id: format!("id_{slug}"),
            item_name: slug.into(),
            sub_type: None,
            quantity,
            price: 0,
            matched_by: "name".into(),
        }
    }

    #[test]
    fn equal_split_when_nothing_is_known_with_remainder_first() {
        assert_eq!(split_platinum(100, &[None, None, None]), vec![34, 33, 33]);
        assert_eq!(split_platinum(70, &[None]), vec![70]);
        assert!(split_platinum(70, &[]).is_empty());
    }

    #[test]
    fn weighted_split_rounds_to_whole_platinum() {
        assert_eq!(split_platinum(105, &[Some(100.0), Some(5.0)]), vec![100, 5]);
        assert_eq!(split_platinum(100, &[Some(2.0), Some(1.0)]), vec![67, 33]);
    }

    #[test]
    fn unknown_weights_take_the_mean_of_the_known_ones() {
        assert_eq!(split_platinum(90, &[Some(20.0), None, Some(40.0)]), vec![20, 30, 40]);
        assert_eq!(split_platinum(90, &[Some(0.0), Some(30.0), Some(30.0)]), vec![30, 30, 30]);
    }

    #[test]
    fn weights_prefer_own_orders_then_medians_and_scale_by_quantity() {
        let items = [item("a", 2), item("b", 1), item("c", 3)];
        let own = |i: &ResolvedItem, ot: OrderType| (i.slug == "a" && ot == OrderType::Sell).then_some(10);
        let median = |i: &ResolvedItem| (i.slug == "b").then_some(7.0);
        assert_eq!(weights_for(&items, Direction::Sale, &own, &median), vec![Some(20.0), Some(7.0), None]);
        assert_eq!(weights_for(&items, Direction::Purchase, &own, &median), vec![None, Some(7.0), None], "buy orders only for purchases");
        let mut items = items;
        price_items(&mut items, 60, &[Some(20.0), Some(7.0), None]);
        assert_eq!(items.iter().map(|i| i.price).collect::<Vec<_>>(), vec![30, 10, 20]);
    }
}
