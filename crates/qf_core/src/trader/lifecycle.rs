//! Pure lifecycle rules: readiness checklist and stop triggers (spec §5.7, amendment C7).

use serde::Serialize;

pub const WS_DOWN_LIMIT_S: i64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Offline,
    Ready,
    Trading,
    Stopping,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Checklist {
    pub token_valid: bool,
    pub ws_connected: bool,
    pub game_data_loaded: bool,
    pub helper_ok: bool,
    pub helper_override: bool,
}

impl Checklist {
    pub fn ready(&self) -> bool {
        self.token_valid && self.ws_connected && self.game_data_loaded && self.helper_ok
    }
}

/// Phase 3 has no helper: the dry-run-only override stands in for it (spec §11 phase 3).
pub fn helper_ok(helper_override: bool, dry_run: bool) -> bool {
    helper_override && dry_run
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum StopReason {
    UserStop,
    SignedOut,
    Unauthorized,
    WebsocketDown,
    HelperLost,
    TraderCritical(String),
    OrderFailures(u32),
    TraderPanic(String),
}

impl StopReason {
    pub fn describe(&self) -> String {
        match self {
            StopReason::UserStop => "Stop button".into(),
            StopReason::SignedOut => "Signed out of warframe.market".into(),
            StopReason::Unauthorized => "warframe.market returned 401 Unauthorized".into(),
            StopReason::WebsocketDown => format!("warframe.market websocket down for more than {} s", WS_DOWN_LIMIT_S),
            StopReason::HelperLost => "Helper unavailable (the helper override needs dry-run)".into(),
            StopReason::TraderCritical(message) => format!("Trader error: {}", message),
            StopReason::OrderFailures(count) => format!("{} consecutive order failures", count),
            StopReason::TraderPanic(message) => format!("Trader panicked: {}", message),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriggerInput {
    pub signed_in: bool,
    pub unauthorized: bool,
    pub ws_down_for_s: Option<i64>,
    pub helper_ok: bool,
}

/// First matching stop trigger while trading. Engine exits are handled by the controller.
pub fn stop_trigger(input: &TriggerInput) -> Option<StopReason> {
    if !input.signed_in {
        Some(StopReason::SignedOut)
    } else if input.unauthorized {
        Some(StopReason::Unauthorized)
    } else if input.ws_down_for_s.is_some_and(|s| s > WS_DOWN_LIMIT_S) {
        Some(StopReason::WebsocketDown)
    } else if !input.helper_ok {
        Some(StopReason::HelperLost)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> TriggerInput {
        TriggerInput { signed_in: true, unauthorized: false, ws_down_for_s: None, helper_ok: true }
    }

    #[test]
    fn ready_needs_every_checklist_item() {
        let all = Checklist { token_valid: true, ws_connected: true, game_data_loaded: true, helper_ok: true, helper_override: true };
        assert!(all.ready());
        for broken in [
            Checklist { token_valid: false, ..all.clone() },
            Checklist { ws_connected: false, ..all.clone() },
            Checklist { game_data_loaded: false, ..all.clone() },
            Checklist { helper_ok: false, ..all.clone() },
        ] {
            assert!(!broken.ready());
        }
    }

    #[test]
    fn helper_override_only_counts_in_dry_run() {
        assert!(helper_ok(true, true));
        assert!(!helper_ok(true, false));
        assert!(!helper_ok(false, true));
    }

    #[test]
    fn stop_triggers_in_priority_order() {
        assert_eq!(stop_trigger(&healthy()), None);
        assert_eq!(stop_trigger(&TriggerInput { signed_in: false, unauthorized: true, ..healthy() }), Some(StopReason::SignedOut));
        assert_eq!(stop_trigger(&TriggerInput { unauthorized: true, ws_down_for_s: Some(99), ..healthy() }), Some(StopReason::Unauthorized));
        assert_eq!(stop_trigger(&TriggerInput { ws_down_for_s: Some(60), ..healthy() }), None, "60 s is allowed");
        assert_eq!(stop_trigger(&TriggerInput { ws_down_for_s: Some(61), helper_ok: false, ..healthy() }), Some(StopReason::WebsocketDown));
        assert_eq!(stop_trigger(&TriggerInput { helper_ok: false, ..healthy() }), Some(StopReason::HelperLost));
    }

    #[test]
    fn stop_reasons_serialize_with_kind_and_detail() {
        assert_eq!(serde_json::to_value(StopReason::OrderFailures(5)).unwrap(), serde_json::json!({"kind": "order_failures", "detail": 5}));
        assert_eq!(serde_json::to_value(StopReason::UserStop).unwrap(), serde_json::json!({"kind": "user_stop"}));
        assert_eq!(StopReason::OrderFailures(5).describe(), "5 consecutive order failures");
    }
}
