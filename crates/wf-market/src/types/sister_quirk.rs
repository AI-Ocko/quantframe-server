use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct SisterQuirk {
    pub id: String,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default = "HashMap::new")]
    pub i18n: HashMap<String, SisterQuirkTranslation>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SisterQuirkTranslation {
    #[serde(rename = "name")]
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub icon: String,
    pub thumb: String,
}
