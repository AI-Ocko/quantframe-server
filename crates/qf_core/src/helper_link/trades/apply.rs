//! Drives the existing stock and wish-list handlers for one trade (amendment E8).

use async_trait::async_trait;
use utils::{Error, OperationSet};
use wf_market::enums::OrderType;

use super::{Direction, ResolvedItem};
use crate::handlers::{handle_item, handle_wish_list};

/// What `handle_wish_list` reports when no wish-list row matched a purchase.
pub const WISH_LIST_NOT_FOUND: &str = "WishListItemBought_NotFound";

/// A purchase asks the wish list first and stops there when nothing matches.
pub fn wish_list_flags(detected_at: &str) -> OperationSet {
    OperationSet::from(vec!["ReturnOn:NotFound".to_string(), format!("SetDate:{detected_at}")])
}

/// A sale skips the WFM check when no stock row matched; a purchase only sets the date.
pub fn item_flags(direction: Direction, detected_at: &str) -> OperationSet {
    match direction {
        Direction::Sale => OperationSet::from(vec!["SkipWFMCheck:ItemSell_NotFound".to_string(), format!("SetDate:{detected_at}")]),
        Direction::Purchase => OperationSet::from(vec![format!("SetDate:{detected_at}")]),
    }
}

#[async_trait]
pub trait ItemApplier: Send + Sync {
    /// `item.price` is the platinum for the whole line.
    async fn apply_item(&self, direction: Direction, item: &ResolvedItem, player: &str, detected_at: &str) -> Result<(), Error>;
}

/// The real handlers. They close or adjust real WFM orders whatever the global dry-run says,
/// the same as the manual "sold" action, because the trade really happened.
pub struct HandlerApplier;

#[async_trait]
impl ItemApplier for HandlerApplier {
    async fn apply_item(&self, direction: Direction, item: &ResolvedItem, player: &str, detected_at: &str) -> Result<(), Error> {
        match direction {
            Direction::Purchase => {
                let (operations, _) = handle_wish_list(
                    item.slug.clone(),
                    &item.sub_type,
                    item.quantity,
                    item.price,
                    player,
                    OrderType::Buy,
                    &wish_list_flags(detected_at),
                )
                .await?;
                if operations.has(WISH_LIST_NOT_FOUND) {
                    handle_item(
                        item.slug.clone(),
                        item.sub_type.clone(),
                        item.quantity,
                        item.price,
                        player,
                        OrderType::Buy,
                        &item_flags(direction, detected_at),
                    )
                    .await?;
                }
            }
            Direction::Sale => {
                handle_item(
                    item.slug.clone(),
                    item.sub_type.clone(),
                    item.quantity,
                    item.price,
                    player,
                    OrderType::Sell,
                    &item_flags(direction, detected_at),
                )
                .await?;
            }
        }
        Ok(())
    }
}

pub struct ApplyFailure {
    /// `<item name> x<quantity>` for every item applied before the failure.
    pub completed: Vec<String>,
    pub error: Error,
}

/// Applies items in order and stops at the first failure.
pub async fn apply_items(
    applier: &dyn ItemApplier,
    direction: Direction,
    items: &[ResolvedItem],
    player: &str,
    detected_at: &str,
) -> Result<(), ApplyFailure> {
    let mut completed = Vec::new();
    for item in items {
        if let Err(error) = applier.apply_item(direction, item, player, detected_at).await {
            return Err(ApplyFailure { completed, error });
        }
        completed.push(format!("{} x{}", item.item_name, item.quantity));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use utils::get_location;

    #[test]
    fn flags_carry_the_date_and_the_per_direction_rules() {
        let date = "2026-09-15T10:00:00Z";
        let wish = wish_list_flags(date);
        assert!(wish.has("ReturnOn:NotFound"));
        assert_eq!(wish.get_value_after("SetDate").as_deref(), Some(date));
        let sale = item_flags(Direction::Sale, date);
        assert_eq!(sale.get_value_after("SkipWFMCheck").as_deref(), Some("ItemSell_NotFound"));
        assert_eq!(sale.get_value_after("SetDate").as_deref(), Some(date));
        let purchase = item_flags(Direction::Purchase, date);
        assert_eq!(purchase.get_value_after("SkipWFMCheck"), None);
        assert_eq!(purchase.get_value_after("SetDate").as_deref(), Some(date));
    }

    struct Recorder {
        fail_on: &'static str,
        seen: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ItemApplier for Recorder {
        async fn apply_item(&self, _direction: Direction, item: &ResolvedItem, _player: &str, _detected_at: &str) -> Result<(), Error> {
            self.seen.lock().unwrap().push(item.slug.clone());
            if item.slug == self.fail_on {
                return Err(Error::new("HandleItem", "boom", get_location!()));
            }
            Ok(())
        }
    }

    fn item(slug: &str) -> ResolvedItem {
        ResolvedItem {
            name: slug.into(),
            slug: slug.into(),
            wfm_id: format!("id_{slug}"),
            item_name: slug.into(),
            sub_type: None,
            quantity: 2,
            price: 10,
            matched_by: "name".into(),
        }
    }

    #[tokio::test]
    async fn apply_items_stops_at_the_first_failure_and_reports_what_was_done() {
        let recorder = Recorder { fail_on: "b", seen: Mutex::new(Vec::new()) };
        let items = [item("a"), item("b"), item("c")];
        let failure = apply_items(&recorder, Direction::Sale, &items, "PlayerA", "2026-09-15T10:00:00Z").await.err().unwrap();
        assert_eq!(failure.completed, vec!["a x2".to_string()]);
        assert_eq!(failure.error.component, "HandleItem");
        assert_eq!(*recorder.seen.lock().unwrap(), vec!["a".to_string(), "b".to_string()], "c is never attempted");

        let ok = Recorder { fail_on: "none", seen: Mutex::new(Vec::new()) };
        assert!(apply_items(&ok, Direction::Purchase, &items, "PlayerA", "2026-09-15T10:00:00Z").await.is_ok());
        assert_eq!(ok.seen.lock().unwrap().len(), 3);
    }
}
