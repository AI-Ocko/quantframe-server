use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DebuggingSettings {
    pub live_scraper: DebuggingLiveScraperSettings,
}

impl Default for DebuggingSettings {
    fn default() -> Self {
        DebuggingSettings {
            live_scraper: DebuggingLiveScraperSettings::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DebuggingLiveScraperSettings {
    pub entries: Vec<serde_json::Value>,
    pub fake_orders: bool,
}

impl Default for DebuggingLiveScraperSettings {
    fn default() -> Self {
        DebuggingLiveScraperSettings {
            entries: Vec::new(),
            fake_orders: false,
        }
    }
}
