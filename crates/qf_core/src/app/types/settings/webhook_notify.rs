use serde::{Deserialize, Serialize};
use serde_json::Value;
use utils::{get_location, info, Error, LoggerOptions};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebHookNotify {
    pub enabled: bool,
    pub url: String,
}
impl WebHookNotify {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            enabled: false,
            url: url.into(),
        }
    }
    pub fn send(&self, value: Value) {
        if self.url.is_empty() {
            return;
        }
        let url = self.url.clone();
        tokio::spawn(async move {
            let client = reqwest::Client::new();
            let res = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header(
                    "User-Agent",
                    format!("Quantframe Server v{}", env!("CARGO_PKG_VERSION")),
                )
                .json(&value)
                .send()
                .await;
            match res {
                Ok(_) => {
                    info(
                        "Helper",
                        &format!("Message sent to webhook: {}", url),
                        &LoggerOptions::default(),
                    );
                }
                Err(e) => {
                    let err = Error::new(
                        "WebhookNotificationError",
                        &format!("{:?}", e),
                        get_location!(),
                    );
                    err.log("webhook_notification.log");
                }
            }
        });
    }
}
