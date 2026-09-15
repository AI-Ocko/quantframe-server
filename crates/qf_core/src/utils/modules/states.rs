use std::sync::{Mutex, OnceLock};

use utils::Error;

use crate::{
    app::{AppState, Settings},
    cache::client::CacheState,
    APP_ERROR,
};

static APP_STATE: OnceLock<Mutex<AppState>> = OnceLock::new();
static CACHE_STATE: OnceLock<Mutex<CacheState>> = OnceLock::new();

pub fn init_app_state(state: AppState) {
    let _ = APP_STATE.set(Mutex::new(state));
}

pub fn init_cache_state(state: CacheState) {
    let _ = CACHE_STATE.set(Mutex::new(state));
}

pub fn app_mutex() -> &'static Mutex<AppState> {
    APP_STATE.get().expect("App state not initialized")
}

pub fn cache_mutex() -> &'static Mutex<CacheState> {
    CACHE_STATE.get().expect("Cache state not initialized")
}

pub fn app_state() -> Result<AppState, Error> {
    Ok(app_mutex().lock()?.clone())
}

/// The app state if it has been initialised (it isn't in unit tests or before startup finishes).
pub fn try_app_state() -> Option<AppState> {
    APP_STATE.get().and_then(|m| m.lock().ok()).map(|app| app.clone())
}

pub fn get_settings() -> Result<Settings, Error> {
    Ok(app_state()?.settings)
}

pub fn cache_client() -> Result<CacheState, Error> {
    Ok(cache_mutex().lock()?.clone())
}

pub fn get_app_error() -> Option<Error> {
    let app_error = APP_ERROR.get_or_init(|| Mutex::new(None));
    let guard = app_error.lock().expect("Failed to lock APP_ERROR");
    guard.clone()
}

pub fn set_app_error(error: Option<Error>) {
    let app_error = APP_ERROR.get_or_init(|| Mutex::new(None));
    let mut guard = app_error.lock().expect("Failed to lock APP_ERROR");
    *guard = error;
}
