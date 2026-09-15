//! The real `TradeEnv`: settings, caches, cached WFM orders, the handlers and notifications (amendment E8).

use std::collections::HashMap;

use serde_json::{json, Value};
use utils::{warning, LoggerOptions};
use wf_market::enums::OrderType;

use super::apply::{HandlerApplier, ItemApplier};
use super::events::{HelperEvent, APPLIED};
use super::resolve::{Overrides, OVERRIDES_FILE, PLATINUM};
use super::sets::{self, SetSource};
use super::{Direction, RawItem, ResolvedItem, TradeEnv};
use crate::cache::types::CacheTradableItem;
use crate::types::UIEvent;
use crate::utils::modules::states;
use crate::utils::SubTypeExt;
use crate::{notify_gui, send_event};

pub struct LiveEnv;

impl TradeEnv for LiveEnv {
    fn auto_trade(&self) -> bool {
        states::try_app_state().is_some_and(|app| app.settings.live_scraper.general.auto_trade)
    }

    fn tradable_items(&self) -> Vec<CacheTradableItem> {
        match states::cache_client().and_then(|cache| cache.tradable_item().get_items()) {
            Ok(items) => items,
            Err(e) => {
                warning("HelperLink:Items", format!("Tradable items unavailable: {}", e.message), &LoggerOptions::default());
                Vec::new()
            }
        }
    }

    fn overrides(&self) -> Overrides {
        Overrides::load(&crate::paths::get().data_dir.join(OVERRIDES_FILE))
    }

    fn own_price(&self, item: &ResolvedItem, order_type: OrderType) -> Option<i64> {
        let app = states::try_app_state()?;
        let sub_type: wf_market::types::SubType = SubTypeExt::from_entity(item.sub_type.clone());
        app.wfm_client.order().cache_orders().find_order(&item.wfm_id, &sub_type, order_type).map(|order| order.platinum as i64)
    }

    fn sets(&self) -> &dyn SetSource {
        sets::cache()
    }

    fn applier(&self) -> &dyn ItemApplier {
        &HandlerApplier
    }

    fn notify(&self, event: &HelperEvent) {
        let values = toast_values(event);
        if event.status == APPLIED {
            let source = json!({"source": "HelperLink:Trade"});
            send_event!(UIEvent::RefreshStockItems, source.clone());
            send_event!(UIEvent::RefreshWishListItems, source.clone());
            send_event!(UIEvent::RefreshTransactions, source);
            notify_gui!("on_trade_event", "green.7", "applied", values, json!({}));
            if let Some(app) = states::try_app_state() {
                app.settings.notifications.on_new_trade.send(
                    &trade_variables(event),
                    Some(json!({"event": "trade", "event_id": event.event_id, "status": event.status})),
                );
            }
        } else {
            notify_gui!("on_trade_event", "yellow", "needs_review", values, json!({"autoClose": false}));
        }
    }
}

fn item_lines(items: &[RawItem]) -> Vec<String> {
    items
        .iter()
        .filter(|item| item.name != PLATINUM)
        .map(|item| match item.rank {
            Some(rank) => format!("{} x{} (rank {rank})", item.name, item.quantity),
            None => format!("{} x{}", item.name, item.quantity),
        })
        .collect()
}

/// The upstream `on_new_trade` variables, filled from a stored event.
pub fn trade_variables(event: &HelperEvent) -> HashMap<String, String> {
    let resolution = event.resolution.clone().unwrap_or_default();
    let offered = item_lines(&event.payload.offered);
    let received = item_lines(&event.payload.received);
    let kind = match resolution.direction {
        Some(Direction::Sale) => "Sale",
        Some(Direction::Purchase) => "Purchase",
        None => "Trade",
    };
    HashMap::from([
        ("<PLAYER_NAME>".to_string(), event.payload.player_name.clone()),
        ("<TIME>".to_string(), event.detected_at.clone()),
        ("<TR_TYPE>".to_string(), kind.to_string()),
        ("<TOTAL_PLAT>".to_string(), resolution.platinum.to_string()),
        ("<OF_COUNT>".to_string(), offered.len().to_string()),
        ("<RE_COUNT>".to_string(), received.len().to_string()),
        ("<OF_ITEMS>".to_string(), offered.join("\n")),
        ("<RE_ITEMS>".to_string(), received.join("\n")),
    ])
}

/// Values for the `on_trade_event.applied` and `on_trade_event.needs_review` toasts.
pub fn toast_values(event: &HelperEvent) -> Value {
    let resolution = event.resolution.clone().unwrap_or_default();
    json!({
        "player_name": event.payload.player_name,
        "direction": resolution.direction.map_or("trade", Direction::as_str),
        "platinum": resolution.platinum,
        "items": resolution.items.iter().map(|item| item.quantity).sum::<i64>(),
        "reason": event.reason.clone().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_link::trades::events::tests::event;
    use crate::helper_link::trades::Resolution;

    fn applied_sale() -> HelperEvent {
        let mut stored = event("e1", "2026-09-15T10:00:00Z", APPLIED);
        stored.resolution = Some(Resolution {
            direction: Some(Direction::Sale),
            platinum: 70,
            items: vec![ResolvedItem {
                name: "Arcane Nullifier".into(),
                slug: "arcane_nullifier".into(),
                wfm_id: "id_arcane_nullifier".into(),
                item_name: "Arcane Nullifier".into(),
                sub_type: Some(utils::SubType::rank(5)),
                quantity: 1,
                price: 70,
                matched_by: "name".into(),
            }],
            extras: vec![],
        });
        stored
    }

    #[test]
    fn trade_variables_follow_the_upstream_template() {
        let vars = trade_variables(&applied_sale());
        assert_eq!(vars["<PLAYER_NAME>"], "PlayerB");
        assert_eq!(vars["<TIME>"], "2026-09-15T10:00:00Z");
        assert_eq!(vars["<TR_TYPE>"], "Sale");
        assert_eq!(vars["<TOTAL_PLAT>"], "70");
        assert_eq!(vars["<OF_ITEMS>"], "Arcane Nullifier x1 (rank 5)");
        assert_eq!((vars["<OF_COUNT>"].as_str(), vars["<RE_COUNT>"].as_str(), vars["<RE_ITEMS>"].as_str()), ("1", "0", ""));
    }

    #[test]
    fn toast_values_name_the_direction_or_fall_back_to_trade() {
        let applied = toast_values(&applied_sale());
        assert_eq!(applied, json!({"player_name": "PlayerB", "direction": "sale", "platinum": 70, "items": 1, "reason": ""}));
        let mut parked = event("e2", "2026-09-15T10:00:00Z", "needs_review");
        parked.reason = Some("no_platinum_side".into());
        let values = toast_values(&parked);
        assert_eq!((values["direction"].as_str(), values["reason"].as_str()), (Some("trade"), Some("no_platinum_side")));
    }
}
