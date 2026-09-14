use utils::{get_location, info, log_json, Error, LoggerOptions};
use wf_market::client::Authenticated as WFAuthenticated;
use wf_market::types::websocket::WsClient;
use wf_market::types::UserPrivate as WFUserPrivate;
use wf_market::Client as WFClient;

use crate::app::modules::ws::setup_socket;
use crate::app::{AppState, User};
use crate::utils::ErrorFromExt;
use crate::{crypto, emit_startup, paths, wfm_account, DATABASE, SENSITIVE_FIELDS};

pub fn update_user(mut cu_user: User, user: &WFUserPrivate) -> User {
    cu_user.anonymous = false;
    cu_user.verification = user.verification;
    cu_user.wfm_banned = user.banned.unwrap_or(false);
    cu_user.wfm_banned_reason = user.ban_message.clone();
    cu_user.wfm_banned_until = user.ban_until.clone();
    cu_user.wfm_id = user.id.to_string();
    cu_user.wfm_username = user.ingame_name.clone();
    cu_user.locale = user.locale.clone();
    cu_user.platform = user.platform.clone();
    cu_user.wfm_avatar = user.avatar.clone();
    cu_user
}

impl AppState {
    pub async fn login(
        &self,
        email: &str,
        password: &str,
    ) -> Result<(WFClient<WFAuthenticated>, User, WsClient), Error> {
        let key = crypto::key()?;
        let device_id = paths::get().device_id()?;
        let wfm_client = self
            .new_base_wfm_client()
            .login(email, password, &device_id)
            .await
            .map_err(|e| Error::from_wfm("AppState:Login", "Failed to login to WFM client", e, get_location!()))?;
        let wfm_user = wfm_client
            .get_user()
            .map_err(|e| Error::from_wfm("AppState:Login", "Failed to get WFM user", e, get_location!()))?;
        wfm_account::save(
            DATABASE.get().expect("Database not initialized"),
            key,
            &wfm_client.get_token(),
            &wfm_user.id.to_string(),
            &wfm_user.ingame_name,
        )
        .await?;
        let updated_user = update_user(self.user.clone(), &wfm_user);
        let ws = setup_socket(wfm_client.clone()).await?;
        updated_user.save()?;
        Ok((wfm_client, updated_user, ws))
    }

    fn new_base_wfm_client(&self) -> WFClient {
        let wfm_client = WFClient::new()
            .with_callback("api:after", |_, data| {
                info(
                    "WarframeMarket:API",
                    &format!(
                        "Method: {} | Route: {} | Took {}ms",
                        data.get_property_value("method", String::new()),
                        data.get_property_value("url", String::new()),
                        data.get_property_value("duration_ms", 0)
                    ),
                    &LoggerOptions::default(),
                );
            })
            .with_callback("api:refresh", |_, data| {
                let state = data.get_property_value("state", String::from("unknown"));
                emit_startup!(format!("wfm.{}", state), json!({}));
            })
            .with_callback("api:error", |_, data| {
                let mut data = data.clone();
                data.mask_sensitive_data(SENSITIVE_FIELDS);
                let timestamp = chrono::Local::now()
                    .with_timezone(&chrono::Utc)
                    .format("%Y_%m_%d_%H_%M_%S")
                    .to_string();

                if let Some(data) = data.properties.clone() {
                    log_json(data, &format!("wfm_api_error_{}.json", timestamp)).ok();
                }
            });
        wfm_client
    }

    pub async fn validate(&mut self) -> Result<WFUserPrivate, Error> {
        let key = crypto::key()?;
        let account = wfm_account::load(DATABASE.get().expect("Database not initialized"), key)
            .await?
            .ok_or_else(|| {
                Error::new(
                    "AppState:Validate",
                    "No warframe.market account stored, please sign in.",
                    get_location!(),
                )
            })?;
        let device_id = paths::get().device_id()?;
        let wfm_client = self
            .new_base_wfm_client()
            .login_with_token(&account.token, &device_id)
            .await
            .map_err(|e| Error::from_wfm("AppState:Validate", "Failed to login with WFM token", e, get_location!()))?;
        let wfm_user = wfm_client
            .get_user()
            .map_err(|e| Error::from_wfm("AppState:Validate", "Failed to get WFM user", e, get_location!()))?;
        let ws = setup_socket(wfm_client.clone()).await?;
        self.wfm_socket = Some(ws);
        self.wfm_client = wfm_client;
        Ok(wfm_user)
    }
}
