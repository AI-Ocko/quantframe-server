use serde_json::json;
use utils::{get_location, info, Error, LoggerOptions};

pub async fn user_set_status(
    status: String) -> Result<(), Error> {
    let app = crate::utils::modules::states::app_mutex();
    let app_state = app.lock()?.clone();
    if app_state.wfm_socket.is_none() {
        return Err(Error::new(
            "User:SetStatus",
            "WebSocket is not connected, please login first.",
            get_location!(),
        ));
    }
    let wfm_socket = app_state.wfm_socket.as_ref().unwrap();
    match wfm_socket.send_request(
        "@wfm|cmd/status/set",
        json!({
            "status": status
        }),
    ) {
        Ok(_) => {
            info(
                "Commands:UserSetStatus",
                &format!("User status set to {}", status),
                &LoggerOptions::default(),
            );
        }
        Err(e) => {
            return Err(Error::new("User:SetStatus", format!("{:?}", e), get_location!()));
        }
    }
    Ok(())
}
