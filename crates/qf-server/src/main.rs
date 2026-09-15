use std::{sync::Arc, time::Duration};

use qf_server::{
    auth::{LoginLimiter, Sessions},
    config::Config,
    routes::{router, ServerState},
};

#[tokio::main]
async fn main() {
    let cfg = Config::from_env().unwrap_or_else(|e| {
        eprintln!("Configuration error: {e}");
        std::process::exit(2);
    });
    let handles = match qf_core::startup::start(cfg.core()).await {
        Ok(handles) => handles,
        Err(e) => {
            eprintln!("Startup failed: {} ({})", e.message, e.component);
            std::process::exit(1);
        }
    };
    let state = ServerState {
        sessions: Arc::new(Sessions::new(Duration::from_secs(30 * 24 * 60 * 60))),
        limiter: Arc::new(LoginLimiter::new(5, Duration::from_secs(60))),
        password_hash: Arc::new(handles.web_password_hash),
        public_origin: Arc::new(cfg.public_origin.clone()),
        web_dir: cfg.web_dir.clone(),
        resources_dir: cfg.resources_dir.clone(),
        data_dir: cfg.data_dir.clone(),
        db: qf_core::DATABASE.get().cloned(),
        trade_env: Arc::new(qf_core::helper_link::trades::live::LiveEnv),
    };
    let listener = tokio::net::TcpListener::bind(&cfg.bind).await.unwrap_or_else(|e| {
        eprintln!("Cannot bind {}: {e}", cfg.bind);
        std::process::exit(1);
    });
    println!("quantframe-server listening on {} (origin {})", cfg.bind, cfg.public_origin);
    axum::serve(listener, router(state)).await.expect("server error");
}
