use std::time::Duration;

use serde::Serialize;

pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(10);
pub const REJECTED_BACKOFF: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Heartbeat {
    pub warframe_running: bool,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Accepted,
    /// 401: the device key is wrong or revoked.
    Rejected,
    Failed(String),
}

impl Outcome {
    pub fn next_delay(&self) -> Duration {
        match self {
            Outcome::Rejected => REJECTED_BACKOFF,
            _ => HEARTBEAT_EVERY,
        }
    }
}

/// One log line for the current state; the loop prints it only when it changes.
pub fn describe(warframe_running: bool, outcome: &Outcome) -> String {
    let game = if warframe_running { "yes" } else { "no" };
    match outcome {
        Outcome::Accepted => format!("Warframe running: {game}; heartbeat accepted"),
        Outcome::Rejected => format!(
            "Warframe running: {game}; device key rejected (401), retrying every {} s. Create a new key in the web UI",
            REJECTED_BACKOFF.as_secs()
        ),
        Outcome::Failed(reason) => format!("Warframe running: {game}; heartbeat failed: {reason}"),
    }
}

pub struct Client {
    http: reqwest::Client,
    url: String,
    key: String,
}

impl Client {
    pub fn new(server_url: &str, device_key: &str) -> Self {
        let http = reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build().expect("HTTP client");
        Self { http, url: format!("{server_url}/helper/heartbeat"), key: device_key.to_string() }
    }

    pub async fn send(&self, heartbeat: &Heartbeat) -> Outcome {
        match self.http.post(&self.url).bearer_auth(&self.key).json(heartbeat).send().await {
            Ok(res) if res.status().is_success() => Outcome::Accepted,
            Ok(res) if res.status() == reqwest::StatusCode::UNAUTHORIZED => Outcome::Rejected,
            Ok(res) => Outcome::Failed(format!("server returned {}", res.status())),
            Err(e) => Outcome::Failed(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::{HeaderMap, StatusCode}, routing::post, Json, Router};
    use std::sync::{Arc, Mutex};

    async fn mock_server() -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = seen.clone();
        let app = Router::new().route(
            "/helper/heartbeat",
            post(move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
                let recorder = recorder.clone();
                async move {
                    if headers.get("authorization").and_then(|v| v.to_str().ok()) != Some("Bearer qfh_good") {
                        return StatusCode::UNAUTHORIZED;
                    }
                    recorder.lock().unwrap().push(body);
                    StatusCode::NO_CONTENT
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, seen)
    }

    fn beat() -> Heartbeat {
        Heartbeat { warframe_running: true, version: "0.1.0".into() }
    }

    #[tokio::test]
    async fn accepted_heartbeats_send_the_expected_json() {
        let (url, seen) = mock_server().await;
        assert_eq!(Client::new(&url, "qfh_good").send(&beat()).await, Outcome::Accepted);
        assert_eq!(*seen.lock().unwrap(), vec![serde_json::json!({"warframe_running": true, "version": "0.1.0"})]);
    }

    #[tokio::test]
    async fn a_wrong_key_is_rejected_and_backs_off() {
        let (url, _) = mock_server().await;
        let outcome = Client::new(&url, "qfh_bad").send(&beat()).await;
        assert_eq!(outcome, Outcome::Rejected);
        assert_eq!(outcome.next_delay(), REJECTED_BACKOFF);
    }

    #[tokio::test]
    async fn an_unreachable_server_is_a_failure_on_the_normal_schedule() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let outcome = Client::new(&url, "qfh_good").send(&beat()).await;
        assert!(matches!(outcome, Outcome::Failed(_)));
        assert_eq!(outcome.next_delay(), HEARTBEAT_EVERY);
    }

    #[test]
    fn state_lines_name_the_game_and_the_outcome() {
        assert_eq!(describe(true, &Outcome::Accepted), "Warframe running: yes; heartbeat accepted");
        assert!(describe(false, &Outcome::Rejected).starts_with("Warframe running: no; device key rejected (401)"));
        assert_eq!(describe(false, &Outcome::Failed("timeout".into())), "Warframe running: no; heartbeat failed: timeout");
    }
}
