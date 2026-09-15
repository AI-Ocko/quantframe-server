//! Pure lifecycle rules: readiness checklist and stop triggers (spec §5.7, amendments C7, D5).

use serde::Serialize;

use crate::helper_link::presence::SILENT_AFTER_S;

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
    /// A qf-helper heartbeat arrived within `READY_WITHIN_S`.
    pub helper_connected: bool,
    /// The latest heartbeat reported Warframe running.
    pub warframe_running: bool,
}

impl Checklist {
    pub fn ready(&self) -> bool {
        self.token_valid && self.ws_connected && self.game_data_loaded && self.helper_connected && self.warframe_running
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum StopReason {
    UserStop,
    SignedOut,
    Unauthorized,
    WebsocketDown,
    HelperSilent,
    WarframeClosed,
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
            StopReason::HelperSilent => format!("No qf-helper heartbeat for more than {} s", SILENT_AFTER_S),
            StopReason::WarframeClosed => "Warframe closed on the gaming PC".into(),
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
    /// `None` when no heartbeat has arrived since the server started.
    pub helper_seconds_since: Option<i64>,
    pub warframe_running: bool,
}

/// First matching stop trigger while trading. Engine exits are handled by the controller.
pub fn stop_trigger(input: &TriggerInput) -> Option<StopReason> {
    if !input.signed_in {
        Some(StopReason::SignedOut)
    } else if input.unauthorized {
        Some(StopReason::Unauthorized)
    } else if input.ws_down_for_s.is_some_and(|s| s > WS_DOWN_LIMIT_S) {
        Some(StopReason::WebsocketDown)
    } else if input.helper_seconds_since.is_none_or(|s| s > SILENT_AFTER_S) {
        Some(StopReason::HelperSilent)
    } else if !input.warframe_running {
        Some(StopReason::WarframeClosed)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> TriggerInput {
        TriggerInput {
            signed_in: true,
            unauthorized: false,
            ws_down_for_s: None,
            helper_seconds_since: Some(5),
            warframe_running: true,
        }
    }

    #[test]
    fn ready_needs_every_checklist_item() {
        let all = Checklist { token_valid: true, ws_connected: true, game_data_loaded: true, helper_connected: true, warframe_running: true };
        assert!(all.ready());
        for broken in [
            Checklist { token_valid: false, ..all.clone() },
            Checklist { ws_connected: false, ..all.clone() },
            Checklist { game_data_loaded: false, ..all.clone() },
            Checklist { helper_connected: false, ..all.clone() },
            Checklist { warframe_running: false, ..all.clone() },
        ] {
            assert!(!broken.ready());
        }
    }

    #[test]
    fn stop_triggers_in_priority_order() {
        assert_eq!(stop_trigger(&healthy()), None);
        assert_eq!(stop_trigger(&TriggerInput { signed_in: false, unauthorized: true, ..healthy() }), Some(StopReason::SignedOut));
        assert_eq!(stop_trigger(&TriggerInput { unauthorized: true, ws_down_for_s: Some(99), ..healthy() }), Some(StopReason::Unauthorized));
        assert_eq!(stop_trigger(&TriggerInput { ws_down_for_s: Some(60), ..healthy() }), None, "60 s is allowed");
        assert_eq!(stop_trigger(&TriggerInput { ws_down_for_s: Some(61), helper_seconds_since: None, ..healthy() }), Some(StopReason::WebsocketDown));
        assert_eq!(stop_trigger(&TriggerInput { helper_seconds_since: None, ..healthy() }), Some(StopReason::HelperSilent));
        assert_eq!(stop_trigger(&TriggerInput { helper_seconds_since: Some(60), ..healthy() }), None, "a heartbeat 60 s old is allowed");
        assert_eq!(stop_trigger(&TriggerInput { helper_seconds_since: Some(61), warframe_running: false, ..healthy() }), Some(StopReason::HelperSilent));
        assert_eq!(stop_trigger(&TriggerInput { warframe_running: false, ..healthy() }), Some(StopReason::WarframeClosed));
    }

    #[test]
    fn stop_reasons_serialize_with_kind_and_detail() {
        assert_eq!(serde_json::to_value(StopReason::OrderFailures(5)).unwrap(), serde_json::json!({"kind": "order_failures", "detail": 5}));
        assert_eq!(serde_json::to_value(StopReason::UserStop).unwrap(), serde_json::json!({"kind": "user_stop"}));
        assert_eq!(serde_json::to_value(StopReason::WarframeClosed).unwrap(), serde_json::json!({"kind": "warframe_closed"}));
        assert_eq!(StopReason::HelperSilent.describe(), "No qf-helper heartbeat for more than 60 s");
    }
}
