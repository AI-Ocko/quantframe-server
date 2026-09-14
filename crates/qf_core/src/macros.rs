/// Macro to emit events with automatic logging
#[macro_export]
macro_rules! emit_event {
    ($event_name:expr, $payload:expr, $log_context:expr) => {{
        let receivers = $crate::events::emit($event_name, $payload);
        ::utils::info(
            &format!("Emit:{}", $log_context),
            &format!("Event: {} ({} receivers)", $event_name, receivers),
            &::utils::LoggerOptions::default(),
        );
    }};
}

#[macro_export]
macro_rules! send_event {
    ($event:expr, $data:expr) => {{
        use serde_json::json;
        use crate::emit_event;
        emit_event!(
            "message",
            json!({ "event": $event.as_str(), "data": $data }),
            format!("SendEvent:{}", $event.as_str())
        );
    }};
}

#[macro_export]
macro_rules! send_event_update {
    ($event:expr, $operation:expr, $data:expr) => {{
        use crate::types::*;
        use serde_json::json;
        use crate::emit_event;
        emit_event!(
            "message_update",
            json!({ "event": $event.as_str(), "operation": $operation.as_str(), "data": $data }),
            format!("SendEventUpdate:{}", $event.as_str())
        );
    }};
}

#[macro_export]
macro_rules! emit_error {
    ($err:expr) => {{
        use crate::send_event;
        use crate::types::*;
        use crate::utils::modules::states;
        send_event!(UIEvent::OnError, Some(json!($err)));
        states::set_app_error(Some($err));
    }};
}

#[macro_export]
macro_rules! clear_error {
    () => {{
        use crate::send_event;
        use crate::types::*;
        use crate::utils::modules::*;
        send_event!(UIEvent::OnError, Some(json!({})));
        states::set_app_error(None);
    }};
}

#[macro_export]
macro_rules! emit_startup {
    ($i18n_key:expr, $Option:expr) => {{
        use crate::types::*;
        use crate::send_event;
        send_event!(UIEvent::OnStartingUp, Some(json!({"i18n_key": $i18n_key, "values": $Option})));
    }};
}

#[macro_export]
macro_rules! emit_update_user {
    ($user:expr) => {{
        use crate::send_event_update;
        send_event_update!(
            UIEvent::UpdateUser,
            UIOperationEvent::CreateOrUpdate,
            Some(json!($user))
        );
    }};
}

#[macro_export]
macro_rules! notify_gui {
    ($i18n_key:expr, $color:expr, $notify_type:expr, $values:expr, $settings:expr) => {{
        use crate::send_event;
        use crate::types::*;
        send_event!(
            UIEvent::OnNotify,
            Some(json!({"i18n_key": $i18n_key, "color": $color, "type": $notify_type, "values": $values, "settings": $settings}))
        );
    }};
}

#[macro_export]
macro_rules! play_sound {
    ($file_name:expr, $volume:expr) => {{
        use crate::emit_event;
        emit_event!(
            "play_sound",
            serde_json::json!({"file_name": $file_name, "volume": $volume}),
            "PlaySound"
        );
    }};
}
