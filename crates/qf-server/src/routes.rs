use std::{path::PathBuf, sync::Arc};

use axum::{
    body::Bytes,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Extension, Path, Request, State,
    },
    http::{header, HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::broadcast::error::RecvError;
use tower_http::services::{ServeDir, ServeFile};

use chrono::Utc;
use qf_core::helper_link::{
    keys::{self, DeviceIdentity},
    presence::{self, Heartbeat},
    trades::{self, IncomingTrade, TradeEnv},
};

use crate::auth::{bearer_from_headers, cleared_cookie, session_cookie, token_from_headers, LoginLimiter, Sessions};

#[derive(Clone)]
pub struct ServerState {
    pub sessions: Arc<Sessions>,
    pub limiter: Arc<LoginLimiter>,
    pub password_hash: Arc<String>,
    pub public_origin: Arc<String>,
    pub web_dir: PathBuf,
    pub resources_dir: PathBuf,
    pub data_dir: PathBuf,
    /// `None` only in tests that don't exercise the helper routes.
    pub db: Option<qf_core::db::DatabaseConnection>,
    /// Resolves and applies helper trades (amendment E8); tests use a fake.
    pub trade_env: Arc<dyn TradeEnv>,
}

pub fn router(state: ServerState) -> Router {
    let spa = ServeDir::new(&state.web_dir).fallback(ServeFile::new(state.web_dir.join("index.html")));
    let protected = Router::new()
        .route("/rpc/{name}", post(rpc))
        .route("/ws", get(ws))
        .nest_service("/sounds/builtin", ServeDir::new(state.resources_dir.join("sounds")))
        .nest_service("/sounds/custom", ServeDir::new(state.data_dir.join("sounds")))
        .fallback_service(spa)
        .layer(middleware::from_fn_with_state(state.clone(), require_session));

    // The helper authenticates with a device key, so it sits outside the Origin check and sessions (amendment D4).
    let helper = Router::new()
        .route("/helper/heartbeat", post(helper_heartbeat))
        .route("/helper/trade", post(helper_trade))
        .layer(middleware::from_fn_with_state(state.clone(), require_device_key));

    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/login", get(login_page).post(login_submit))
        .route("/logout", post(logout))
        .merge(protected)
        .layer(middleware::from_fn_with_state(state.clone(), check_origin))
        .merge(helper)
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .with_state(state)
}

async fn require_device_key(State(state): State<ServerState>, mut req: Request, next: Next) -> Response {
    let Some(db) = state.db.as_ref() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "Database not ready").into_response();
    };
    let unauthorized = || {
        (StatusCode::UNAUTHORIZED, Json(json!({"component": "Helper", "message": "Invalid or revoked device key"})))
            .into_response()
    };
    let Some(key) = bearer_from_headers(req.headers()) else { return unauthorized() };
    match keys::authenticate(db, &key, Utc::now()).await {
        Ok(Some(device)) => {
            req.extensions_mut().insert(device);
            next.run(req).await
        }
        Ok(None) => unauthorized(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error)).into_response(),
    }
}

async fn helper_heartbeat(Extension(device): Extension<DeviceIdentity>, Json(heartbeat): Json<Heartbeat>) -> StatusCode {
    presence::get().record(&device.name, heartbeat, Utc::now());
    StatusCode::NO_CONTENT
}

/// Amendment E4. The body is parsed by hand so a wrong shape is 400, as the helper expects.
async fn helper_trade(State(state): State<ServerState>, Extension(device): Extension<DeviceIdentity>, body: Bytes) -> Response {
    let bad_request =
        |message: String| (StatusCode::BAD_REQUEST, Json(json!({"component": "Helper", "message": message}))).into_response();
    let incoming: IncomingTrade = match serde_json::from_slice(&body) {
        Ok(incoming) => incoming,
        Err(e) => return bad_request(format!("Invalid trade body: {e}")),
    };
    if let Err(message) = trades::validate(&incoming) {
        return bad_request(message);
    }
    let Some(db) = state.db.as_ref() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "Database not ready").into_response();
    };
    match trades::handle_incoming(db, state.trade_env.as_ref(), &device.name, incoming, Utc::now()).await {
        Ok(outcome) => (StatusCode::OK, Json(outcome)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error)).into_response(),
    }
}

async fn check_origin(State(state): State<ServerState>, req: Request, next: Next) -> Response {
    let is_read = req.method() == Method::GET || req.method() == Method::HEAD;
    if !is_read || req.uri().path() == "/ws" {
        let allowed = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|origin| origin == state.public_origin.as_str());
        if !allowed {
            return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
        }
    }
    next.run(req).await
}

async fn require_session(State(state): State<ServerState>, req: Request, next: Next) -> Response {
    let valid = token_from_headers(req.headers()).is_some_and(|t| state.sessions.is_valid(&t));
    if valid {
        return next.run(req).await;
    }
    let path = req.uri().path();
    if path.starts_with("/rpc/") || path == "/ws" || path.starts_with("/sounds/") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Redirect::to("/login").into_response()
}

async fn login_page() -> Html<&'static str> {
    Html(include_str!("login.html"))
}

#[derive(Deserialize)]
struct LoginForm {
    password: String,
}

async fn login_submit(State(state): State<ServerState>, Form(form): Form<LoginForm>) -> Response {
    if !state.limiter.try_acquire() {
        return (StatusCode::TOO_MANY_REQUESTS, "Too many login attempts; wait a minute.").into_response();
    }
    if !qf_core::web_auth::verify_password(&form.password, &state.password_hash) {
        return Redirect::to("/login?error=1").into_response();
    }
    let token = state.sessions.create();
    ([(header::SET_COOKIE, session_cookie(&token))], Redirect::to("/")).into_response()
}

async fn logout(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    if let Some(token) = token_from_headers(&headers) {
        state.sessions.remove(&token);
    }
    ([(header::SET_COOKIE, cleared_cookie())], Redirect::to("/login")).into_response()
}

async fn rpc(Path(name): Path<String>, body: Bytes) -> Response {
    let args: Value = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"component": "Rpc", "message": format!("Invalid JSON body: {e}")})),
                )
                    .into_response()
            }
        }
    };
    match qf_core::commands::rpc::dispatch(&name, args).await {
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"component": "Rpc", "message": format!("Unknown command {name}")})),
        )
            .into_response(),
        Some(Ok(value)) => Json(value).into_response(),
        Some(Err(error)) => (StatusCode::UNPROCESSABLE_ENTITY, Json(error)).into_response(),
    }
}

async fn ws(upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(forward_events)
}

async fn forward_events(mut socket: WebSocket) {
    let mut events = qf_core::events::subscribe();
    loop {
        tokio::select! {
            frame = events.recv() => match frame {
                Ok(value) => {
                    if socket.send(Message::Text(value.to_string().into())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            },
            incoming = socket.recv() => match incoming {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => {}
            },
        }
    }
}
