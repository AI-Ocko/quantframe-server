use crate::{app::Settings, HAS_STARTED};
use serde_json::{json, Value};
use utils::Error;

pub async fn initialized() -> Result<bool, Error> {
    let started = HAS_STARTED.get().cloned().unwrap_or(false);
    return Ok(started);
}
pub async fn app_get_app_info() -> Result<Value, Error> {
    let app = crate::utils::modules::states::app_mutex();
    let app = app.lock()?;
    Ok(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "name": "Quantframe Server",
        "description": "Headless server version of Quantframe",
        "authors": "Kenya-DK (upstream)",
        "is_dev": app.is_development,
        "use_temp_db": app.use_temp_db,
        "tos_uuid": app.settings.tos_uuid.clone(),
        "is_pre_release": app.is_pre_release,
    }))
}

pub async fn app_get_settings() -> Result<Settings, Error> {
    let app = crate::utils::modules::states::app_mutex();
    let app = app.lock()?;
    Ok(app.settings.clone())
}

pub async fn app_get_default_settings() -> Result<Settings, Error> {
    Ok(Settings::default())
}

pub async fn app_update_settings(mut settings: Settings) -> Result<Settings, Error> {
    let app = crate::utils::modules::states::app_mutex();
    let mut app = app.lock()?;
    settings.notifications.custom_sounds = app.settings.notifications.custom_sounds.clone();
    app.update_settings(settings.clone())?;
    Ok(settings.clone())
}

pub async fn app_accept_tos(
    id: String) -> Result<(), Error> {
    let app = crate::utils::modules::states::app_mutex();
    let mut app = app.lock()?;
    let mut settings = app.settings.clone();
    settings.tos_uuid = id.clone();
    app.update_settings(settings)?;
    Ok(())
}
pub async fn app_notify_reset(id: String) -> Result<Value, Error> {
    let value = json!(crate::app::NotificationsSetting::default());
    if value[id.clone()].is_object() {
        return Ok(value[id.clone()].clone());
    }
    Ok(json!({}))
}
