#![allow(non_snake_case)]
#![allow(deprecated)]

use std::sync::{Mutex, OnceLock};

use service::sea_orm::DatabaseConnection;
use ::utils::Error;

pub mod app;
pub mod cache;
pub mod crypto;
pub mod db;
pub mod enums;
pub mod events;
pub mod game_data;
pub mod handlers;
pub mod helper;
mod macros;
pub mod paths;
pub mod types;
pub mod utils;
pub mod web_auth;
pub mod wfm_account;

pub static DATABASE: OnceLock<DatabaseConnection> = OnceLock::new();
pub static HAS_STARTED: OnceLock<bool> = OnceLock::new();
pub static APP_ERROR: OnceLock<Mutex<Option<Error>>> = OnceLock::new();
pub static SENSITIVE_FIELDS: &[&str] = &[
    "email",
    "password",
    "authorization",
    "wfm_token",
    "webhook",
    "slug",
    "device_key",
    "token_ciphertext",
];
