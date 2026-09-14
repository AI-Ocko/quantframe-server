use std::sync::OnceLock;

use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;

static SENDER: OnceLock<broadcast::Sender<Value>> = OnceLock::new();

fn sender() -> &'static broadcast::Sender<Value> {
    SENDER.get_or_init(|| broadcast::channel(1024).0)
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
}
