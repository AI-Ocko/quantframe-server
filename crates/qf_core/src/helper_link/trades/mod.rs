//! Trade events reported by `qf-helper` (spec §5.8, amendments E1–E9).

pub mod apply;
pub mod events;
pub mod live;
pub mod resolve;
pub mod sets;
pub mod split;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use service::sea_orm::DatabaseConnection;
use utils::{get_location, warning, Error, LoggerOptions, SubType};
use wf_market::enums::OrderType;

pub use qf_log_parser::{RawItem, RawTrade};

use crate::cache::types::CacheTradableItem;
use crate::collector::ts;
use crate::trader::price_source::key_of;
use apply::{apply_items, ItemApplier};
use events::{HelperEvent, APPLIED, IGNORED, NEEDS_REVIEW};
use resolve::{resolve_trade, ItemIndex, Overrides};
use sets::{fold_sets, set_candidates, SetSource};
use split::{medians, price_items, weights_for};

/// `status` for a replayed event_id; never stored (amendment E4).
pub const DUPLICATE: &str = "duplicate";
/// Reason while an event is being applied; left behind only if the server stops mid-way.
pub const APPLYING: &str = "applying";
pub const AUTO_TRADE_OFF: &str = "auto_trade_off";
pub const NO_GOODS: &str = "no_goods";
pub const NO_PLATINUM_SIDE: &str = "no_platinum_side";
pub const REVIEWED: &str = "reviewed";

/// Body of `POST /helper/trade` (amendment E4).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IncomingTrade {
    pub event_id: String,
    /// RFC 3339, from the helper's clock.
    pub detected_at: String,
    pub trade: RawTrade,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Purchase,
    Sale,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Purchase => "purchase",
            Direction::Sale => "sale",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedItem {
    /// The in-game name as reported.
    pub name: String,
    pub slug: String,
    pub wfm_id: String,
    pub item_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<SubType>,
    pub quantity: i64,
    /// Platinum for the whole line, the way the handlers expect it.
    pub price: i64,
    /// `name`, `override`, `set` or `review` (amendment E5).
    pub matched_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Resolution {
    #[serde(default)]
    pub direction: Option<Direction>,
    #[serde(default)]
    pub platinum: i64,
    #[serde(default)]
    pub items: Vec<ResolvedItem>,
    /// Non-platinum items on the platinum side; recorded, never applied (amendment E6).
    #[serde(default)]
    pub extras: Vec<RawItem>,
}

/// Reply to `POST /helper/trade` (amendment E4).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Outcome {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Outcome {
    fn duplicate() -> Self {
        Self { status: DUPLICATE.into(), reason: None }
    }

    fn of(event: &HelperEvent) -> Self {
        Self { status: event.status.clone(), reason: event.reason.clone() }
    }
}

/// Everything the pipeline needs besides the database. `live::LiveEnv` is the real one.
pub trait TradeEnv: Send + Sync {
    /// `live_scraper.general.auto_trade`, the kill switch (amendment E8).
    fn auto_trade(&self) -> bool;
    fn tradable_items(&self) -> Vec<CacheTradableItem>;
    fn overrides(&self) -> Overrides;
    /// Price of the user's own cached WFM order for this item, sub type and order type.
    fn own_price(&self, item: &ResolvedItem, order_type: OrderType) -> Option<i64>;
    fn sets(&self) -> &dyn SetSource;
    fn applier(&self) -> &dyn ItemApplier;
    /// Called once for each stored outcome: applied, needs_review or apply_failed.
    fn notify(&self, event: &HelperEvent);
}

/// One row of the Review modal (amendment E9).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReviewItem {
    pub slug: String,
    #[serde(default)]
    pub sub_type: Option<SubType>,
    pub quantity: i64,
    pub price: i64,
}

/// Checks the E4 body beyond its shape. Returns `detected_at` in UTC.
pub fn validate(incoming: &IncomingTrade) -> Result<DateTime<Utc>, String> {
    let id = &incoming.event_id;
    if id.len() != 64 || !id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
        return Err("event_id must be 64 lowercase hex characters".into());
    }
    DateTime::parse_from_rfc3339(&incoming.detected_at)
        .map(|at| at.with_timezone(&Utc))
        .map_err(|e| format!("detected_at is not RFC 3339: {e}"))
}

async fn price(conn: &DatabaseConnection, env: &dyn TradeEnv, resolution: &mut Resolution) -> Result<(), Error> {
    let Some(direction) = resolution.direction else { return Ok(()) };
    let medians = medians(conn, &resolution.items).await?;
    let weights = weights_for(
        &resolution.items,
        direction,
        &|item, order_type| env.own_price(item, order_type),
        &|item| medians.get(&(item.wfm_id.clone(), key_of(&item.sub_type))).copied(),
    );
    price_items(&mut resolution.items, resolution.platinum, &weights);
    Ok(())
}

/// Classify, resolve, fold sets and price (amendments E6–E8). The second value is the review
/// reason; `None` means the event can be applied.
pub async fn resolve_event(conn: &DatabaseConnection, env: &dyn TradeEnv, trade: &RawTrade) -> Result<(Resolution, Option<String>), Error> {
    let index = ItemIndex::from_items(env.tradable_items());
    let resolved = match resolve_trade(trade, &index, &env.overrides()) {
        Ok(resolved) => resolved,
        Err(reason) => return Ok((Resolution::default(), Some(reason))),
    };
    let mut resolution = resolved.resolution;
    let candidates = set_candidates(&resolution.items, &index);
    if !candidates.is_empty() {
        let parts = env.sets().parts_for(&candidates, &index).await;
        resolution.items = fold_sets(resolution.items, &candidates, &parts, &index);
    }
    price(conn, env, &mut resolution).await?;
    let reason = if !resolved.unresolved.is_empty() {
        Some(format!("unresolved: {}", resolved.unresolved.join(", ")))
    } else if resolution.items.is_empty() {
        Some(NO_GOODS.to_string())
    } else if !env.auto_trade() {
        Some(AUTO_TRADE_OFF.to_string())
    } else {
        None
    };
    Ok((resolution, reason))
}

/// Applies `resolution.items` and records the result on `event`. The outer error is the database;
/// the inner one is the handler failure, already recorded as `apply_failed`.
async fn apply_and_record(
    conn: &DatabaseConnection,
    env: &dyn TradeEnv,
    event: &mut HelperEvent,
    direction: Direction,
    resolution: Resolution,
    reviewed_at: Option<DateTime<Utc>>,
) -> Result<Result<(), Error>, Error> {
    match apply_items(env.applier(), direction, &resolution.items, &event.payload.player_name, &event.detected_at).await {
        Ok(()) => {
            events::set_status(conn, &event.event_id, APPLIED, None, Some(&resolution), reviewed_at).await?;
            event.status = APPLIED.into();
            event.reason = None;
            event.resolution = Some(resolution);
            event.reviewed_at = reviewed_at.map(ts);
            env.notify(event);
            Ok(Ok(()))
        }
        Err(failure) => {
            let reason = format!("apply_failed: {}", failure.error.component);
            let done = if failure.completed.is_empty() { "nothing".to_string() } else { failure.completed.join(", ") };
            warning(
                "HelperLink:Trade",
                format!("Trade {} with {} stopped: {}. Already applied: {done}", event.event_id, event.payload.player_name, failure.error.message),
                &LoggerOptions::default(),
            );
            events::set_status(conn, &event.event_id, NEEDS_REVIEW, Some(&reason), Some(&resolution), None).await?;
            event.status = NEEDS_REVIEW.into();
            event.reason = Some(reason);
            event.resolution = Some(resolution);
            env.notify(event);
            Ok(Err(failure.error))
        }
    }
}

/// `POST /helper/trade`, processed inside the request (amendments E4–E8).
pub async fn handle_incoming(
    conn: &DatabaseConnection,
    env: &dyn TradeEnv,
    device_name: &str,
    incoming: IncomingTrade,
    now: DateTime<Utc>,
) -> Result<Outcome, Error> {
    let detected_at = validate(&incoming).map_err(|message| Error::new("HelperLink:Trade", message, get_location!()))?;
    if events::exists(conn, &incoming.event_id).await? {
        return Ok(Outcome::duplicate());
    }
    let (resolution, review_reason) = resolve_event(conn, env, &incoming.trade).await?;
    let mut event = HelperEvent {
        event_id: incoming.event_id,
        device_name: device_name.to_string(),
        received_at: ts(now),
        detected_at: ts(detected_at),
        status: NEEDS_REVIEW.into(),
        reason: Some(review_reason.clone().unwrap_or_else(|| APPLYING.to_string())),
        payload: incoming.trade,
        resolution: Some(resolution.clone()),
        reviewed_at: None,
    };
    if let Err(error) = events::insert(conn, &event).await {
        // A concurrent request with the same event_id won the insert.
        if events::exists(conn, &event.event_id).await? {
            return Ok(Outcome::duplicate());
        }
        return Err(error);
    }
    match (review_reason, resolution.direction) {
        (None, Some(direction)) => {
            // A handler failure is already recorded on the event; the helper just hears needs_review.
            let _ = apply_and_record(conn, env, &mut event, direction, resolution, None).await?;
        }
        _ => env.notify(&event),
    }
    Ok(Outcome::of(&event))
}

fn review_error(message: impl Into<String>) -> Error {
    Error::new("HelperLink:Review", message, get_location!())
}

async fn reviewable(conn: &DatabaseConnection, event_id: &str) -> Result<HelperEvent, Error> {
    let event = events::get(conn, event_id).await?.ok_or_else(|| review_error(format!("Unknown trade event {event_id}")))?;
    if event.status != NEEDS_REVIEW {
        return Err(review_error(format!("This trade is already {}; only trades that need review can change", event.status)));
    }
    Ok(event)
}

/// `helper_trade_apply` (amendment E9): the user's items, the stored direction, player and time.
pub async fn apply_reviewed(
    conn: &DatabaseConnection,
    env: &dyn TradeEnv,
    event_id: &str,
    items: Vec<ReviewItem>,
    now: DateTime<Utc>,
) -> Result<HelperEvent, Error> {
    let mut event = reviewable(conn, event_id).await?;
    let mut resolution = event.resolution.clone().unwrap_or_default();
    let Some(direction) = resolution.direction else {
        return Err(review_error("This trade has no platinum side, so it can only be ignored"));
    };
    if items.is_empty() {
        return Err(review_error("Add at least one item"));
    }
    let index = ItemIndex::from_items(env.tradable_items());
    let mut resolved = Vec::with_capacity(items.len());
    for item in items {
        let Some(found) = index.by_slug(&item.slug) else {
            return Err(review_error(format!("Unknown item {}", item.slug)));
        };
        if item.quantity < 1 || item.price < 0 {
            return Err(review_error(format!("{}: quantity must be at least 1 and price at least 0", found.name)));
        }
        resolved.push(ResolvedItem {
            name: found.name.clone(),
            slug: found.wfm_url.clone(),
            wfm_id: found.wfm_id.clone(),
            item_name: found.name.clone(),
            sub_type: item.sub_type,
            quantity: item.quantity,
            price: item.price,
            matched_by: "review".into(),
        });
    }
    resolution.items = resolved;
    apply_and_record(conn, env, &mut event, direction, resolution, Some(now)).await??;
    Ok(event)
}

/// `helper_trade_ignore` (amendment E9).
pub async fn ignore(conn: &DatabaseConnection, event_id: &str, now: DateTime<Utc>) -> Result<HelperEvent, Error> {
    let mut event = reviewable(conn, event_id).await?;
    events::set_status(conn, event_id, IGNORED, Some(REVIEWED), event.resolution.as_ref(), Some(now)).await?;
    event.status = IGNORED.into();
    event.reason = Some(REVIEWED.into());
    event.reviewed_at = Some(ts(now));
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use utils::get_location;

    use crate::helper_link::trades::events::tests::sale_trade;
    use crate::helper_link::trades::resolve::tests::items;
    use crate::helper_link::trades::sets::PartsMap;
    use crate::trader::store::tests::db;

    #[derive(Default)]
    struct Fake {
        auto_trade: bool,
        parts: PartsMap,
        fail_on: Option<String>,
        /// (direction, slug, quantity, price, player, detected_at)
        applied: Mutex<Vec<(Direction, String, i64, i64, String, String)>>,
        notified: Mutex<Vec<(String, Option<String>)>>,
    }

    fn fake(auto_trade: bool) -> Fake {
        Fake {
            auto_trade,
            parts: PartsMap::from([(
                "wolf_sledge_set".to_string(),
                vec!["wolf_sledge_blueprint".into(), "wolf_sledge_motor".into(), "wolf_sledge_head".into(), "wolf_sledge_handle".into()],
            )]),
            ..Default::default()
        }
    }

    #[async_trait]
    impl ItemApplier for Fake {
        async fn apply_item(&self, direction: Direction, item: &ResolvedItem, player: &str, detected_at: &str) -> Result<(), Error> {
            if self.fail_on.as_deref() == Some(item.slug.as_str()) {
                return Err(Error::new("HandleItem", "WFM rejected the order", get_location!()));
            }
            self.applied.lock().unwrap().push((direction, item.slug.clone(), item.quantity, item.price, player.into(), detected_at.into()));
            Ok(())
        }
    }

    impl TradeEnv for Fake {
        fn auto_trade(&self) -> bool {
            self.auto_trade
        }
        fn tradable_items(&self) -> Vec<CacheTradableItem> {
            items()
        }
        fn overrides(&self) -> Overrides {
            Overrides::default()
        }
        fn own_price(&self, _item: &ResolvedItem, _order_type: OrderType) -> Option<i64> {
            None
        }
        fn sets(&self) -> &dyn SetSource {
            &self.parts
        }
        fn applier(&self) -> &dyn ItemApplier {
            self
        }
        fn notify(&self, event: &HelperEvent) {
            self.notified.lock().unwrap().push((event.status.clone(), event.reason.clone()));
        }
    }

    fn raw(name: &str, quantity: i64, rank: Option<i64>) -> RawItem {
        RawItem { name: name.into(), quantity, rank }
    }

    fn trade(offered: Vec<RawItem>, received: Vec<RawItem>) -> RawTrade {
        RawTrade { player_name: "PlayerA".into(), ee_timestamp: "422.424".into(), offered, received }
    }

    fn incoming(id: char, trade: RawTrade) -> IncomingTrade {
        IncomingTrade { event_id: id.to_string().repeat(64), detected_at: "2026-09-15T12:00:00+02:00".into(), trade }
    }

    fn now() -> DateTime<Utc> {
        crate::collector::parse_ts("2026-09-15T10:00:05Z").unwrap()
    }

    fn outcome(status: &str, reason: Option<&str>) -> Outcome {
        Outcome { status: status.into(), reason: reason.map(str::to_string) }
    }

    #[test]
    fn validate_wants_lowercase_hex_ids_and_rfc3339_times() {
        let ok = incoming('a', sale_trade());
        assert_eq!(validate(&ok).unwrap(), crate::collector::parse_ts("2026-09-15T10:00:00Z").unwrap());
        assert!(validate(&IncomingTrade { event_id: "A".repeat(64), ..ok.clone() }).is_err());
        assert!(validate(&IncomingTrade { event_id: "a".repeat(63), ..ok.clone() }).is_err());
        assert!(validate(&IncomingTrade { event_id: "g".repeat(64), ..ok.clone() }).is_err());
        assert!(validate(&IncomingTrade { detected_at: "yesterday".into(), ..ok }).is_err());
    }

    #[tokio::test]
    async fn a_resolved_sale_is_applied_with_the_whole_platinum_and_the_game_time() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let result = handle_incoming(&conn, &env, "gaming-pc", incoming('a', sale_trade()), now()).await.unwrap();
        assert_eq!(result, outcome(events::APPLIED, None));
        assert_eq!(
            *env.applied.lock().unwrap(),
            vec![(Direction::Sale, "arcane_nullifier".to_string(), 1, 70, "PlayerB".to_string(), "2026-09-15T10:00:00Z".to_string())]
        );
        let stored = events::get(&conn, &"a".repeat(64)).await.unwrap().unwrap();
        assert_eq!((stored.status.as_str(), stored.reason.as_deref()), (events::APPLIED, None));
        assert_eq!((stored.device_name.as_str(), stored.received_at.as_str()), ("gaming-pc", "2026-09-15T10:00:05Z"));
        let resolution = stored.resolution.unwrap();
        assert_eq!(resolution.items[0].sub_type, Some(SubType::rank(5)));
        assert_eq!(*env.notified.lock().unwrap(), vec![(events::APPLIED.to_string(), None)]);
    }

    #[tokio::test]
    async fn a_purchase_of_every_part_applies_the_set() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let parts = trade(
            vec![raw("Platinum", 30, None)],
            vec![raw("Wolf Sledge Blueprint", 1, None), raw("Wolf Sledge Motor", 1, None), raw("Wolf Sledge Head", 1, None), raw("Wolf Sledge Handle", 1, None)],
        );
        assert_eq!(handle_incoming(&conn, &env, "gaming-pc", incoming('b', parts), now()).await.unwrap().status, events::APPLIED);
        let applied = env.applied.lock().unwrap();
        assert_eq!(applied.len(), 1);
        assert_eq!((applied[0].0, applied[0].1.as_str(), applied[0].2, applied[0].3), (Direction::Purchase, "wolf_sledge_set", 1, 30));
        let stored = events::get(&conn, &"b".repeat(64)).await.unwrap().unwrap();
        assert_eq!(stored.resolution.unwrap().items[0].matched_by, "set");
    }

    #[tokio::test]
    async fn unresolved_names_park_the_event_without_applying() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let mixed = trade(vec![raw("Platinum", 40, None)], vec![raw("Wolf Sledge Handle", 1, None), raw("Mystery Thing", 2, None)]);
        let result = handle_incoming(&conn, &env, "gaming-pc", incoming('c', mixed), now()).await.unwrap();
        assert_eq!(result, outcome(events::NEEDS_REVIEW, Some("unresolved: Mystery Thing")));
        assert!(env.applied.lock().unwrap().is_empty());
        let stored = events::get(&conn, &"c".repeat(64)).await.unwrap().unwrap();
        let resolution = stored.resolution.unwrap();
        assert_eq!((resolution.direction, resolution.platinum, resolution.items.len()), (Some(Direction::Purchase), 40, 1));
        assert_eq!(*env.notified.lock().unwrap(), vec![(events::NEEDS_REVIEW.to_string(), Some("unresolved: Mystery Thing".to_string()))]);
    }

    #[tokio::test]
    async fn auto_trade_off_parks_every_event() {
        let (_dir, conn) = db().await;
        let env = fake(false);
        let result = handle_incoming(&conn, &env, "gaming-pc", incoming('d', sale_trade()), now()).await.unwrap();
        assert_eq!(result, outcome(events::NEEDS_REVIEW, Some(AUTO_TRADE_OFF)));
        assert!(env.applied.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn no_platinum_side_and_platinum_only_trades_are_parked() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let swap = trade(vec![raw("Adaptation", 1, Some(10))], vec![raw("Primed Firestorm", 1, Some(10))]);
        assert_eq!(handle_incoming(&conn, &env, "gaming-pc", incoming('e', swap), now()).await.unwrap(), outcome(events::NEEDS_REVIEW, Some(NO_PLATINUM_SIDE)));
        let stored = events::get(&conn, &"e".repeat(64)).await.unwrap().unwrap();
        assert_eq!(stored.resolution.unwrap().direction, None);

        let gift = trade(vec![raw("Platinum", 5, None)], vec![]);
        assert_eq!(handle_incoming(&conn, &env, "gaming-pc", incoming('f', gift), now()).await.unwrap(), outcome(events::NEEDS_REVIEW, Some(NO_GOODS)));
        assert!(env.applied.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_known_event_id_is_a_duplicate_and_changes_nothing() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        handle_incoming(&conn, &env, "gaming-pc", incoming('1', sale_trade()), now()).await.unwrap();
        let again = handle_incoming(&conn, &env, "gaming-pc", incoming('1', sale_trade()), now()).await.unwrap();
        assert_eq!(again, outcome(DUPLICATE, None));
        assert_eq!(env.applied.lock().unwrap().len(), 1);
        assert_eq!(events::list(&conn, None, 1, 10).await.unwrap().total, 1);
    }

    #[tokio::test]
    async fn invalid_input_is_an_error_and_stores_nothing() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let bad = IncomingTrade { event_id: "nope".into(), ..incoming('2', sale_trade()) };
        assert!(handle_incoming(&conn, &env, "gaming-pc", bad, now()).await.is_err());
        assert_eq!(events::list(&conn, None, 1, 10).await.unwrap().total, 0);
    }

    #[tokio::test]
    async fn a_failing_handler_parks_the_event_as_apply_failed_and_keeps_the_resolution() {
        let (_dir, conn) = db().await;
        let env = Fake { fail_on: Some("arcane_nullifier".into()), ..fake(true) };
        let two = trade(vec![raw("Platinum", 100, None)], vec![raw("Adaptation", 1, Some(10)), raw("Arcane Nullifier", 1, Some(5))]);
        let result = handle_incoming(&conn, &env, "gaming-pc", incoming('3', two), now()).await.unwrap();
        assert_eq!(result, outcome(events::NEEDS_REVIEW, Some("apply_failed: HandleItem")));
        let applied = env.applied.lock().unwrap();
        assert_eq!((applied.len(), applied[0].1.as_str(), applied[0].3), (1, "adaptation", 50));
        let stored = events::get(&conn, &"3".repeat(64)).await.unwrap().unwrap();
        assert_eq!(stored.resolution.unwrap().items.len(), 2);
    }

    #[tokio::test]
    async fn review_applies_the_given_items_once_and_marks_the_event_reviewed() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let mixed = trade(vec![raw("Platinum", 40, None)], vec![raw("Wolf Sledge Handle", 1, None), raw("Mystery Thing", 2, None)]);
        handle_incoming(&conn, &env, "gaming-pc", incoming('4', mixed), now()).await.unwrap();
        let id = "4".repeat(64);
        let items = vec![
            ReviewItem { slug: "wolf_sledge_handle".into(), sub_type: None, quantity: 1, price: 10 },
            ReviewItem { slug: "adaptation".into(), sub_type: Some(SubType::rank(10)), quantity: 2, price: 30 },
        ];
        let reviewed = apply_reviewed(&conn, &env, &id, items.clone(), now()).await.unwrap();
        assert_eq!((reviewed.status.as_str(), reviewed.reason.as_deref()), (events::APPLIED, None));
        assert_eq!(reviewed.reviewed_at.as_deref(), Some("2026-09-15T10:00:05Z"));
        let resolution = reviewed.resolution.clone().unwrap();
        assert!(resolution.items.iter().all(|i| i.matched_by == "review"));
        assert_eq!(resolution.items[1].item_name, "Adaptation");
        assert_eq!(events::get(&conn, &id).await.unwrap().unwrap(), reviewed);
        assert_eq!(
            env.applied.lock().unwrap().iter().map(|a| (a.0, a.1.clone(), a.2, a.3)).collect::<Vec<_>>(),
            vec![(Direction::Purchase, "wolf_sledge_handle".to_string(), 1, 10), (Direction::Purchase, "adaptation".to_string(), 2, 30)]
        );

        assert!(apply_reviewed(&conn, &env, &id, items, now()).await.is_err(), "an applied event can't be applied again");
        assert!(ignore(&conn, &id, now()).await.is_err(), "nor ignored");
        assert_eq!(env.applied.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn review_is_refused_without_a_direction_or_for_bad_items_and_ignore_works() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let swap = trade(vec![raw("Adaptation", 1, Some(10))], vec![raw("Primed Firestorm", 1, Some(10))]);
        handle_incoming(&conn, &env, "gaming-pc", incoming('5', swap), now()).await.unwrap();
        let swap_id = "5".repeat(64);
        let one = vec![ReviewItem { slug: "adaptation".into(), sub_type: None, quantity: 1, price: 0 }];
        assert!(apply_reviewed(&conn, &env, &swap_id, one.clone(), now()).await.is_err(), "no platinum side");

        let parked = trade(vec![raw("Platinum", 40, None)], vec![raw("Mystery Thing", 1, None)]);
        handle_incoming(&conn, &env, "gaming-pc", incoming('6', parked), now()).await.unwrap();
        let parked_id = "6".repeat(64);
        let unknown = vec![ReviewItem { slug: "nope".into(), sub_type: None, quantity: 1, price: 40 }];
        assert!(apply_reviewed(&conn, &env, &parked_id, unknown, now()).await.is_err());
        let zero = vec![ReviewItem { slug: "adaptation".into(), sub_type: None, quantity: 0, price: 40 }];
        assert!(apply_reviewed(&conn, &env, &parked_id, zero, now()).await.is_err());
        assert!(apply_reviewed(&conn, &env, &parked_id, vec![], now()).await.is_err());
        assert!(apply_reviewed(&conn, &env, &"7".repeat(64), one, now()).await.is_err(), "unknown event");
        assert_eq!(events::get(&conn, &parked_id).await.unwrap().unwrap().status, events::NEEDS_REVIEW);
        assert!(env.applied.lock().unwrap().is_empty());

        let ignored = ignore(&conn, &swap_id, now()).await.unwrap();
        assert_eq!((ignored.status.as_str(), ignored.reason.as_deref()), (events::IGNORED, Some(REVIEWED)));
        assert_eq!(events::get(&conn, &swap_id).await.unwrap().unwrap(), ignored);
    }
}
