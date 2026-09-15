use utils::{get_location, Error, LogLevel, OperationSet};
use wf_market::client::Authenticated as WFAuthenticated;
use wf_market::enums::ApiVersion;
use wf_market::types::websocket::{WsClient, WsMessage};
use wf_market::Client as WFClient;

use crate::utils::modules::states;
use crate::{clear_error, emit_error, emit_update_user, HAS_STARTED};

fn send_ws_state(key: impl Into<String>, data: &WsMessage) {
    let key = key.into();
    let mut current_error = match states::get_app_error().as_mut() {
        Some(err) => err.clone(),
        None => Error::new("WebSocket", "Connection state", get_location!())
            .set_log_level(LogLevel::Warning),
    };

    if current_error.component != "WebSocket" {
        return;
    }

    current_error
        .properties
        .merge_properties(data.payload.clone(), true, true);

    let mut operations = OperationSet::from(
        current_error
            .properties
            .get_property_value::<Vec<String>>("operations", vec![]),
    );

    let ws_type = key.split(':').next().unwrap_or("unknown").to_string();
    operations.remove_prefix(&ws_type);
    operations.add(&key);

    current_error
        .properties
        .set_property_value("operations", operations.operations.clone());

    if !operations.ends_with("Disconnected") {
        clear_error!();
        return;
    }
    current_error.log("websocket_info.log");
    emit_error!(current_error);
}

fn update_user_status(states: impl Into<String>) {
    let states = states.into();
    let app = states::app_mutex();
    let mut guard = app.lock().expect("Failed to lock notification state");
    guard.user.wfm_status = states;
    guard.user.save().expect("Failed to save user status");
    emit_update_user!(json!({"wfm_status": guard.user.wfm_status}));
}

pub async fn setup_socket(
    wfm_client: WFClient<WFAuthenticated>,
) -> Result<WsClient, Error> {
    if wfm_client.get_user().is_err() {
        return Err(Error::new(
            "AppState:SetupSocket",
            "WFM client user is not authenticated, please login first.",
            get_location!(),
        ));
    }

    let ws_client = wfm_client
        .create_websocket(ApiVersion::V2)
        .set_log_unhandled(true)
        .register_callback("internal/connected", move |msg, _, _| {
            crate::trader::session::get().set_ws_connected(true, chrono::Utc::now());
            send_ws_state("Main:Connected", msg);
            Ok(())
        })
        .unwrap()
        .register_callback("internal/disconnected", move |msg, _, _| {
            crate::trader::session::get().set_ws_connected(false, chrono::Utc::now());
            send_ws_state("Main:Disconnected", msg);
            Ok(())
        })
        .unwrap()
        .register_callback("internal/reconnecting", move |msg, _, _| {
            crate::trader::session::get().set_ws_connected(false, chrono::Utc::now());
            send_ws_state("Main:Disconnected", msg);
            Ok(())
        })
        .unwrap()
        .register_callback("event/account/banned", move |msg, _, _| {
            let payload = msg.clone().payload.unwrap();
            emit_update_user!(json!({
                "wfm_banned": true,
                "wfm_banned_reason": payload["banMessage"].as_str().unwrap_or("").to_string(),
                "wfm_banned_until": payload["banUntil"].as_str().unwrap_or("").to_string()
            }));
            Ok(())
        })
        .unwrap()
        .register_callback("event/account/banLifted", move |_, _, _| {
            emit_update_user!(json!({"wfm_banned": false}));
            Ok(())
        })
        .unwrap()
        .register_callback("event/reports/online", move |_, _, _| Ok(()))
        .unwrap()
        .register_callback("cmd/status/set:ok", move |msg, _, _| {
            match msg.payload.as_ref() {
                Some(payload) => update_user_status(
                    payload["status"]
                        .as_str()
                        .unwrap_or("invisible")
                        .to_string(),
                ),

                None => {}
            }
            Ok(())
        })
        .unwrap()
        .register_callback("event/status/set", move |msg, _, _| {
            if !HAS_STARTED.get().cloned().unwrap_or(false) {
                return Ok(());
            }
            match msg.payload.as_ref() {
                Some(payload) => update_user_status(
                    payload["status"]
                        .as_str()
                        .unwrap_or("invisible")
                        .to_string(),
                ),

                None => {}
            }
            Ok(())
        })
        .unwrap()
        .build()
        .await
        .unwrap();
    Ok(ws_client)
}
