use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use qf_log_parser::RawTrade;
use serde::Deserialize;

use crate::config::Config;
use crate::ee_log::{Tail, POLL_EVERY};
use crate::heartbeat::REJECTED_BACKOFF;
use crate::queue::{Queue, QueuedEvent};

pub const RETRY_EVERY: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq)]
pub enum TradeOutcome {
    /// 2xx with the server's status: applied, needs_review, ignored or duplicate.
    Accepted(String),
    /// 401: the device key is wrong or revoked.
    Rejected,
    /// A 4xx the server will never accept; the event is dropped.
    Dropped(String),
    /// Network trouble or a 5xx; retried.
    Failed(String),
}

impl TradeOutcome {
    pub fn removes_from_queue(&self) -> bool {
        matches!(self, TradeOutcome::Accepted(_) | TradeOutcome::Dropped(_))
    }

    pub fn next_delay(&self) -> Option<Duration> {
        match self {
            TradeOutcome::Rejected => Some(REJECTED_BACKOFF),
            TradeOutcome::Failed(_) => Some(RETRY_EVERY),
            _ => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            TradeOutcome::Accepted(status) => status.clone(),
            TradeOutcome::Rejected => "device key rejected (401)".into(),
            TradeOutcome::Dropped(reason) => format!("dropped ({reason})"),
            TradeOutcome::Failed(reason) => format!("failed ({reason})"),
        }
    }
}

#[derive(Deserialize)]
struct TradeResponse {
    status: String,
}

pub struct TradeClient {
    http: reqwest::Client,
    url: String,
    key: String,
}

impl TradeClient {
    pub fn new(server_url: &str, device_key: &str) -> Self {
        let http = reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build().expect("HTTP client");
        Self { http, url: format!("{server_url}/helper/trade"), key: device_key.to_string() }
    }

    pub async fn send(&self, event: &QueuedEvent) -> TradeOutcome {
        use reqwest::StatusCode;
        match self.http.post(&self.url).bearer_auth(&self.key).json(event).send().await {
            Ok(res) if res.status().is_success() => match res.json::<TradeResponse>().await {
                Ok(body) => TradeOutcome::Accepted(body.status),
                Err(e) => TradeOutcome::Accepted(format!("accepted, unreadable reply: {e}")),
            },
            Ok(res) if res.status() == StatusCode::UNAUTHORIZED => TradeOutcome::Rejected,
            Ok(res)
                if matches!(
                    res.status(),
                    StatusCode::BAD_REQUEST
                        | StatusCode::NOT_FOUND
                        | StatusCode::CONFLICT
                        | StatusCode::PAYLOAD_TOO_LARGE
                        | StatusCode::UNPROCESSABLE_ENTITY
                ) =>
            {
                TradeOutcome::Dropped(format!("server returned {}", res.status()))
            }
            Ok(res) => TradeOutcome::Failed(format!("server returned {}", res.status())),
            Err(e) => TradeOutcome::Failed(e.to_string()),
        }
    }
}

fn platinum_of(items: &[qf_log_parser::RawItem]) -> i64 {
    items.iter().filter(|i| i.name == "Platinum").map(|i| i.quantity).sum()
}

/// `trade detected: sale 70p with PlayerB, 1 items; server: applied` (amendment E3).
pub fn describe(trade: &RawTrade, server: &str) -> String {
    let offered = platinum_of(&trade.offered);
    let received = platinum_of(&trade.received);
    let (kind, platinum, goods) = match (offered > 0, received > 0) {
        (true, false) => ("purchase", offered, trade.received.len()),
        (false, true) => ("sale", received, trade.offered.len()),
        _ => ("unknown", offered.max(received), trade.offered.len() + trade.received.len()),
    };
    format!("trade detected: {kind} {platinum}p with {}, {goods} items; server: {server}", trade.player_name)
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Appends everything still pending to the queue, oldest first, keeping whatever could not be written.
/// The tail never replays a dialog, so an event dropped here would be lost for good.
fn persist(queue: &Queue, pending: &mut Vec<QueuedEvent>) {
    pending.retain(|queued| match queue.push(queued) {
        Ok(()) => {
            println!("{}", describe(&queued.trade, "queued"));
            false
        }
        Err(e) => {
            eprintln!("cannot queue trade: {e}");
            true
        }
    });
}

/// Polls EE.log every second, queues successes and drains the queue oldest first.
pub async fn run(config: Config, queue: Queue) -> ! {
    let client = TradeClient::new(&config.server_url, &config.device_key);
    let mut tail = Tail::start_at_end(&config.ee_log_path);
    println!("watching {} from byte {} ({} queued trade(s))", config.ee_log_path.display(), tail.offset(), queue.len());
    let mut wait_until: Option<Instant> = None;
    let mut pending: Vec<QueuedEvent> = Vec::new();
    loop {
        for event in tail.poll() {
            pending.push(QueuedEvent { event_id: event.event_id, detected_at: now_rfc3339(), trade: event.trade });
        }
        persist(&queue, &mut pending);
        if wait_until.is_none_or(|until| Instant::now() >= until) {
            wait_until = None;
            loop {
                let next = match queue.peek() {
                    Ok(Some(next)) => next,
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("cannot read the trade queue: {e}");
                        wait_until = Some(Instant::now() + RETRY_EVERY);
                        break;
                    }
                };
                let outcome = client.send(&next).await;
                println!("{}", describe(&next.trade, &outcome.label()));
                if outcome.removes_from_queue() {
                    if let Err(e) = queue.pop() {
                        eprintln!("cannot update the trade queue: {e}");
                        wait_until = Some(Instant::now() + RETRY_EVERY);
                        break;
                    }
                }
                if let Some(delay) = outcome.next_delay() {
                    wait_until = Some(Instant::now() + delay);
                    break;
                }
            }
        }
        tokio::time::sleep(POLL_EVERY).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::Path, http::{HeaderMap, StatusCode}, response::IntoResponse, routing::post, Json, Router};
    use qf_log_parser::RawItem;
    use std::sync::{Arc, Mutex};

    fn event(id: &str) -> QueuedEvent {
        QueuedEvent {
            event_id: id.into(),
            detected_at: "2026-09-15T10:00:00Z".into(),
            trade: RawTrade {
                player_name: "PlayerB".into(),
                ee_timestamp: "1170.388".into(),
                offered: vec![RawItem { name: "Arcane Nullifier".into(), quantity: 1, rank: Some(5) }],
                received: vec![RawItem { name: "Platinum".into(), quantity: 70, rank: None }],
            },
        }
    }

    /// Replies according to the event id: `ok-*` 200, `dup-*` 200 duplicate, `bad-*` 422, `boom-*` 500.
    async fn mock_server() -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = seen.clone();
        let app = Router::new().route(
            "/helper/trade",
            post(move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
                let recorder = recorder.clone();
                async move {
                    if headers.get("authorization").and_then(|v| v.to_str().ok()) != Some("Bearer qfh_good") {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    recorder.lock().unwrap().push(body.clone());
                    let id = body["event_id"].as_str().unwrap_or("");
                    if id.starts_with("bad-") {
                        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
                    }
                    if id.starts_with("boom-") {
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                    let status = if id.starts_with("dup-") { "duplicate" } else { "applied" };
                    Json(serde_json::json!({ "status": status })).into_response()
                }
            }),
        );
        let _ = Path::<String>::from; // keeps the import used if axum changes the prelude
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, seen)
    }

    #[tokio::test]
    async fn accepted_events_send_the_e4_body_and_return_the_status() {
        let (url, seen) = mock_server().await;
        let client = TradeClient::new(&url, "qfh_good");
        assert_eq!(client.send(&event("ok-1")).await, TradeOutcome::Accepted("applied".into()));
        assert_eq!(client.send(&event("dup-1")).await, TradeOutcome::Accepted("duplicate".into()));
        let body = &seen.lock().unwrap()[0];
        assert_eq!(body["event_id"], "ok-1");
        assert_eq!(body["detected_at"], "2026-09-15T10:00:00Z");
        assert_eq!(body["trade"]["player_name"], "PlayerB");
        assert_eq!(body["trade"]["offered"][0]["rank"], 5);
        assert_eq!(body["trade"]["received"][0]["quantity"], 70);
    }

    #[tokio::test]
    async fn outcomes_decide_queue_removal_and_delay() {
        let (url, _) = mock_server().await;
        let rejected = TradeClient::new(&url, "qfh_bad").send(&event("ok-2")).await;
        assert_eq!(rejected, TradeOutcome::Rejected);
        assert!(!rejected.removes_from_queue());
        assert_eq!(rejected.next_delay(), Some(REJECTED_BACKOFF));

        let client = TradeClient::new(&url, "qfh_good");
        let dropped = client.send(&event("bad-1")).await;
        assert!(matches!(dropped, TradeOutcome::Dropped(_)));
        assert!(dropped.removes_from_queue());
        assert_eq!(dropped.next_delay(), None);

        let failed = client.send(&event("boom-1")).await;
        assert!(matches!(failed, TradeOutcome::Failed(_)));
        assert!(!failed.removes_from_queue());
        assert_eq!(failed.next_delay(), Some(RETRY_EVERY));
    }

    #[test]
    fn events_that_cannot_be_queued_are_retried_on_the_next_tick() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the queue's directory belongs: create_dir_all, and so push, fails.
        let blocker = dir.path().join("state");
        std::fs::write(&blocker, "").unwrap();
        let queue = Queue::new(blocker.join("qf-helper/trade-queue.jsonl"));
        let mut pending = vec![event("ok-1"), event("ok-2")];

        persist(&queue, &mut pending);
        assert_eq!(pending.len(), 2, "events the queue rejected are kept for the next tick");
        assert_eq!(queue.len(), 0);

        std::fs::remove_file(&blocker).unwrap();
        persist(&queue, &mut pending);
        assert!(pending.is_empty(), "the retry drains the kept events");
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.peek().unwrap().unwrap().event_id, "ok-1", "oldest first");
    }

    #[test]
    fn descriptions_name_the_direction_platinum_and_player() {
        assert_eq!(describe(&event("x").trade, "applied"), "trade detected: sale 70p with PlayerB, 1 items; server: applied");
        let mut purchase = event("y").trade;
        std::mem::swap(&mut purchase.offered, &mut purchase.received);
        assert_eq!(describe(&purchase, "queued"), "trade detected: purchase 70p with PlayerB, 1 items; server: queued");
    }
}
