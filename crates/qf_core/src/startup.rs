use std::path::PathBuf;

use utils::{error, info, init_logger, set_base_path, warning, Error, LoggerOptions};

use crate::app::AppState;
use crate::cache::CacheState;
use crate::crypto::{self, SecretKey};
use crate::paths::{self, Paths};
use crate::utils::modules::states;
use crate::utils::OrderListExt;
use crate::{db, game_data, web_auth, DATABASE, HAS_STARTED};

pub struct CoreConfig {
    pub data_dir: PathBuf,
    pub resources_dir: PathBuf,
    pub secret_key_hex: Option<String>,
    pub web_password_file: PathBuf,
    pub collector_enabled: bool,
}

pub struct CoreHandles {
    pub web_password_hash: String,
}

pub async fn start(cfg: CoreConfig) -> Result<CoreHandles, Error> {
    paths::init(Paths::new(&cfg.data_dir, &cfg.resources_dir)?);
    init_logger();
    set_base_path(paths::get().logs_dir().to_string_lossy().to_string());

    let conn = db::connect(&paths::get().data_dir).await?;
    let _ = DATABASE.set(conn);
    let conn = DATABASE.get().expect("Database just set");

    let web_password_hash = web_auth::ensure_password(conn, &cfg.web_password_file).await?;

    let key = match cfg.secret_key_hex.as_deref() {
        Some(hex) => Some(SecretKey::from_hex(hex)?),
        None => {
            warning(
                "Startup",
                "QF_SECRET_KEY_FILE not readable; warframe.market sign-in is disabled",
                &LoggerOptions::default(),
            );
            None
        }
    };
    crypto::init_key(key);
    crate::market::gate::install();

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| Error::new("Startup:Http", e.to_string(), utils::get_location!()))?;
    let items = game_data::load_items(&paths::get().cache_dir(), &http).await?;
    let cache = CacheState::new(paths::get().cache_dir());
    cache.load(items)?;
    states::init_cache_state(cache);

    states::init_app_state(AppState::new(false).await?);
    {
        let cache = states::cache_client()?;
        let app = states::app_mutex().lock()?;
        app.wfm_client.order().cache_orders_mut().apply_item_info(&cache)?;
    }
    if states::app_state()?.wfm_socket.is_some() {
        crate::trader::session::get().mark_signed_in(
            chrono::Utc::now(),
            crypto::jwt_expiry(&states::app_state()?.wfm_client.get_token()),
        );
        if let Err(e) = crate::commands::user::user_set_status("invisible".to_string()).await {
            error(
                "Startup",
                format!("Could not force invisible status: {:?}", e),
                &LoggerOptions::default(),
            );
        }
    }

    crate::collector::runner::start(crate::collector::runner::CollectorStart {
        conn: conn.clone(),
        cache_dir: paths::get().cache_dir(),
        enabled: cfg.collector_enabled,
    })
    .await?;

    let _ = HAS_STARTED.set(true);
    info("Startup", "Core started", &LoggerOptions::default());
    Ok(CoreHandles { web_password_hash })
}
