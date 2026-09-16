use std::sync::OnceLock;

use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;

static SENDER: OnceLock<broadcast::Sender<Value>> = OnceLock::new();

fn sender() -> &'static broadcast::Sender<Value> {
    // Log frames share this channel with the UI events (spec §20 L2), so the buffer is sized
    // so a browser stalled for a while loses log lines before it loses trader/stock frames;
    // `/ws` drops on lag.
    SENDER.get_or_init(|| broadcast::channel(8192).0)
}

pub fn subscribe() -> broadcast::Receiver<Value> {
    sender().subscribe()
}

/// Sends `{channel, payload}` to every connected browser. Returns how many receivers got it.
pub fn emit(channel: &str, payload: impl Serialize) -> usize {
    let payload = serde_json::to_value(payload).unwrap_or(Value::Null);
    sender()
        .send(json!({ "channel": channel, "payload": payload }))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn emitted_frames_reach_subscribers() {
        let mut rx = subscribe();
        let receivers = emit("message", serde_json::json!({"event": "User:Update", "data": 1}));
        assert!(receivers >= 1);
        let frame = rx.recv().await.unwrap();
        assert_eq!(frame["channel"], "message");
        assert_eq!(frame["payload"]["event"], "User:Update");
    }

    #[test]
    fn a_log_frame_carries_level_and_line() {
        let mut rx = subscribe();
        crate::startup::log_sink(&utils::LogLevel::Warning, "[2026-09-16 05:49:33] [3.3] [WARNING] [Test] hello");
        // Every test in this binary shares the broadcast channel, so skip frames from other channels.
        let frame = std::iter::from_fn(|| rx.try_recv().ok())
            .find(|frame| frame["channel"] == "log")
            .expect("one log frame");
        assert_eq!(frame["payload"]["level"], "WARNING");
        assert!(frame["payload"]["line"].as_str().unwrap().ends_with("hello"));
    }
}
