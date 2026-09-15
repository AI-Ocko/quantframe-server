use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use chrono::Utc;
use http_body_util::BodyExt;
use qf_core::helper_link::{keys, presence};
use qf_server::{
    auth::{LoginLimiter, Sessions},
    routes::{router, ServerState},
};
use tower::ServiceExt;

const ORIGIN: &str = "http://test.local";
const PASSWORD: &str = "correct horse battery";

fn app() -> (Router, ServerState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("web")).unwrap();
    std::fs::write(dir.path().join("web/index.html"), "<html>app</html>").unwrap();
    let state = ServerState {
        sessions: Arc::new(Sessions::new(Duration::from_secs(3600))),
        limiter: Arc::new(LoginLimiter::new(5, Duration::from_secs(60))),
        password_hash: Arc::new(qf_core::web_auth::hash_password(PASSWORD).unwrap()),
        public_origin: Arc::new(ORIGIN.to_string()),
        web_dir: dir.path().join("web"),
        resources_dir: dir.path().join("resources"),
        data_dir: dir.path().to_path_buf(),
        db: None,
    };
    (router(state.clone()), state, dir)
}

fn rpc(name: &str, cookie: Option<&str>, origin: &str) -> Request<Body> {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri(format!("/rpc/{name}"))
        .header(header::ORIGIN, origin)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = cookie {
        req = req.header(header::COOKIE, format!("qf_session={token}"));
    }
    req.body(Body::from("{}")).unwrap()
}

fn login(password: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri("/login")
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("password={}", password.replace(' ', "+"))))
        .unwrap()
}

#[tokio::test]
async fn healthz_needs_no_session() {
    let (app, _, _dir) = app();
    let res = app.oneshot(Request::get("/healthz").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn rpc_without_session_is_unauthorized() {
    let (app, _, _dir) = app();
    let res = app.oneshot(rpc("initialized", None, ORIGIN)).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn wrong_origin_is_forbidden_even_with_session() {
    let (app, state, _dir) = app();
    let token = state.sessions.create();
    let res = app.oneshot(rpc("initialized", Some(&token), "http://evil.example")).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn login_sets_cookie_only_for_correct_password() {
    let (app, _, _dir) = app();
    let bad = app.clone().oneshot(login("nope")).await.unwrap();
    assert_eq!(bad.status(), StatusCode::SEE_OTHER);
    assert_eq!(bad.headers()[header::LOCATION], "/login?error=1");
    assert!(bad.headers().get(header::SET_COOKIE).is_none());

    let good = app.oneshot(login(PASSWORD)).await.unwrap();
    assert_eq!(good.status(), StatusCode::SEE_OTHER);
    let cookie = good.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with("qf_session="));
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
}

#[tokio::test]
async fn sixth_login_attempt_in_a_minute_is_rate_limited() {
    let (app, _, _dir) = app();
    for _ in 0..5 {
        app.clone().oneshot(login("nope")).await.unwrap();
    }
    let res = app.oneshot(login(PASSWORD)).await.unwrap();
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn app_page_redirects_to_login_without_session() {
    let (app, _, _dir) = app();
    let res = app.oneshot(Request::get("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(res.headers()[header::LOCATION], "/login");
}

#[tokio::test]
async fn session_serves_app_and_dispatches_rpc() {
    let (app, state, _dir) = app();
    let token = state.sessions.create();

    let page = app
        .clone()
        .oneshot(
            Request::get("/stock")
                .header(header::COOKIE, format!("qf_session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let body = page.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"<html>app</html>");

    let unknown = app.clone().oneshot(rpc("does_not_exist", Some(&token), ORIGIN)).await.unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    let ok = app.oneshot(rpc("initialized", Some(&token), ORIGIN)).await.unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    let body = ok.into_body().collect().await.unwrap().to_bytes();
    assert!(&body[..] == b"false" || &body[..] == b"true");
}

async fn helper_app() -> (Router, ServerState, tempfile::TempDir) {
    let (_, mut state, dir) = app();
    state.db = Some(qf_core::db::connect(dir.path()).await.unwrap());
    (router(state.clone()), state, dir)
}

fn heartbeat(key: Option<&str>, body: &str) -> Request<Body> {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/helper/heartbeat")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(key) = key {
        req = req.header(header::AUTHORIZATION, format!("Bearer {key}"));
    }
    req.body(Body::from(body.to_string())).unwrap()
}

const BEAT: &str = r#"{"warframe_running": true, "version": "0.1.0"}"#;

#[tokio::test]
async fn heartbeat_requires_a_valid_device_key() {
    let (app, _, _dir) = helper_app().await;
    let missing = app.clone().oneshot(heartbeat(None, BEAT)).await.unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    let wrong = app.oneshot(heartbeat(Some("qfh_0000"), BEAT)).await.unwrap();
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn heartbeat_needs_no_origin_or_session_and_records_presence() {
    let (app, state, _dir) = helper_app().await;
    let db = state.db.as_ref().unwrap();
    let created = keys::create(db, "http-test-pc", Utc::now()).await.unwrap();

    let res = app.oneshot(heartbeat(Some(&created.key), BEAT)).await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let snap = presence::get().snapshot(Utc::now());
    assert!(snap.connected && snap.warframe_running);
    assert_eq!(snap.device_name.as_deref(), Some("http-test-pc"));
    assert!(keys::list(db).await.unwrap()[0].last_seen_at.is_some());
}

#[tokio::test]
async fn revoked_keys_are_rejected() {
    let (app, state, _dir) = helper_app().await;
    let db = state.db.as_ref().unwrap();
    let created = keys::create(db, "revoked-pc", Utc::now()).await.unwrap();
    keys::revoke(db, created.device.id, Utc::now()).await.unwrap();
    let res = app.oneshot(heartbeat(Some(&created.key), BEAT)).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn heartbeat_body_must_say_whether_warframe_is_running() {
    let (app, state, _dir) = helper_app().await;
    let created = keys::create(state.db.as_ref().unwrap(), "bad-body-pc", Utc::now()).await.unwrap();
    let res = app.oneshot(heartbeat(Some(&created.key), r#"{"version": "0.1.0"}"#)).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
