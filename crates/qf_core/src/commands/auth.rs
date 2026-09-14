use utils::{get_location, info, Error, LoggerOptions};

use crate::{
    app::User,
    send_event,
    types::UIEvent,
    utils::{modules::states, OrderListExt},
};

pub async fn auth_me() -> Result<User, Error> {
    let app = states::app_mutex();
    let app = app.lock()?;
    let mut user = app.user.clone();
    user.wfm_token = String::new(); // Do not expose the token
    Ok(user)
}

pub async fn auth_login(email: String, password: String) -> Result<User, Error> {
    let app = states::app_mutex();
    let cache = states::cache_client()?;
    let app_state = app.lock()?.clone();
    let (wfm_client, updated_user, ws) = app_state
        .login(&email, &password)
        .await
        .map_err(|e| e.log("auth_login.log"))?;
    info(
        "Commands:AuthLogin",
        &format!("User {} logged in successfully", updated_user.wfm_username),
        &LoggerOptions::default(),
    );
    wfm_client.order().cache_orders_mut().apply_item_info(&cache)?;
    let mut app = app.lock()?;
    app.wfm_client = wfm_client;
    app.user = updated_user.clone();
    app.wfm_socket = Some(ws);
    send_event!(UIEvent::RefreshCache, "Cache refreshed successfully");
    Ok(updated_user)
}

pub async fn auth_logout() -> Result<User, Error> {
    let app = states::app_mutex();
    let app_state = app.lock()?.clone();
    if let Some(ws) = &app_state.wfm_socket {
        if let Err(e) = ws.disconnect() {
            let err = Error::new(
                "Commands:AuthLogout",
                format!("Failed to close WebSocket: {:?}", e),
                get_location!(),
            );
            err.log("auth_logout.log");
            return Err(err);
        }
    }
    crate::wfm_account::delete(crate::DATABASE.get().expect("Database not initialized")).await?;
    let new_user = User::default();
    new_user.save()?;
    let mut app = app.lock()?;
    app.user = new_user.clone();
    app.wfm_socket = None;
    Ok(new_user)
}
