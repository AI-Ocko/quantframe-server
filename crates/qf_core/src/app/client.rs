use utils::{Error, LogLevel};
use wf_market::Client as WFClient;

use crate::app::modules::auth::update_user;
use crate::app::{AppState, Settings, User};

impl AppState {
    pub async fn new(use_temp_db: bool) -> Result<Self, Error> {
        let user = User::load().unwrap_or_else(|e| {
            e.log("app_init.log");
            User::default()
        });
        let settings = Settings::load().unwrap_or_else(|e| {
            e.log("app_init.log");
            Settings::default()
        });
        let mut state = AppState {
            wfm_client: WFClient::new_default("", "N/A")
                .await
                .expect("Failed to create WFM client"),
            user,
            is_development: cfg!(debug_assertions),
            use_temp_db,
            is_pre_release: false,
            settings,
            wfm_socket: None,
        };
        match state.validate().await {
            Ok(wfm_user) => {
                state.user = update_user(state.user, &wfm_user);
            }
            Err(e) => {
                e.log("user_validation.log");
                if e.log_level != LogLevel::Warning {
                    state.user = User::default();
                }
            }
        }
        state.user.save()?;
        Ok(state)
    }

    pub fn update_settings(&mut self, settings: Settings) -> Result<(), Error> {
        self.settings = settings;
        self.settings.save()?;
        Ok(())
    }
}
