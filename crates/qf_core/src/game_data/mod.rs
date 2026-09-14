use std::{collections::HashMap, path::Path};

use serde::{Deserialize, Serialize};
use utils::{get_location, info, warning, Error, LoggerOptions};

use crate::cache::types::{CacheTradableItem, SubType};

pub const WFM_ITEMS_URL: &str = "https://api.warframe.market/v2/items";
pub const LAST_GOOD_FILE: &str = "wfm_items.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WfmItemI18n {
    pub name: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub thumb: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WfmItem {
    pub id: String,
    pub slug: String,
    #[serde(default)]
    pub game_ref: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub i18n: HashMap<String, WfmItemI18n>,
    pub max_rank: Option<i64>,
    pub bulk_tradable: Option<bool>,
    pub subtypes: Option<Vec<String>>,
    pub max_amber_stars: Option<i64>,
    pub max_cyan_stars: Option<i64>,
    pub req_mastery_rank: Option<i64>,
}

#[derive(Deserialize)]
struct ItemsResponse {
    data: Vec<WfmItem>,
}

pub fn parse_items_response(json: &str) -> Result<Vec<WfmItem>, Error> {
    serde_json::from_str::<ItemsResponse>(json)
        .map(|r| r.data)
        .map_err(|e| Error::new("GameData:Parse", format!("Invalid /v2/items response: {}", e), get_location!()))
}

pub fn to_tradable_item(item: &WfmItem) -> Option<CacheTradableItem> {
    let en = item.i18n.get("en")?;
    let has_sub_type = item.max_rank.is_some()
        || item.subtypes.is_some()
        || item.max_amber_stars.is_some()
        || item.max_cyan_stars.is_some();
    Some(CacheTradableItem {
        name: en.name.clone(),
        unique_name: item.game_ref.clone(),
        wfm_id: item.id.clone(),
        wfm_url: item.slug.clone(),
        trade_tax: 0,
        mr_requirement: item.req_mastery_rank.unwrap_or(0),
        tags: item.tags.clone(),
        icon: en.icon.clone(),
        bulk_tradable: item.bulk_tradable.unwrap_or(false),
        sub_type: has_sub_type.then(|| SubType {
            max_rank: item.max_rank,
            variants: item.subtypes.clone(),
            amber_stars: item.max_amber_stars,
            cyan_stars: item.max_cyan_stars,
        }),
        variant_to_unique_name: HashMap::new(),
    })
}

pub async fn load_items(cache_dir: &Path, http: &reqwest::Client) -> Result<Vec<CacheTradableItem>, Error> {
    load_items_from(cache_dir, http, WFM_ITEMS_URL).await
}

/// Fetches the item list, saving it as the last good copy. Falls back to that copy when the fetch fails.
pub async fn load_items_from(
    cache_dir: &Path,
    http: &reqwest::Client,
    url: &str,
) -> Result<Vec<CacheTradableItem>, Error> {
    let last_good = cache_dir.join(LAST_GOOD_FILE);
    let fetched: Result<String, String> = async {
        let body = http
            .get(url)
            .header("Language", "en")
            .header("Platform", "pc")
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        parse_items_response(&body).map_err(|e| e.message.clone())?;
        Ok(body)
    }
    .await;

    let body = match fetched {
        Ok(body) => {
            if let Err(e) = std::fs::write(&last_good, &body) {
                warning(
                    "GameData:Load",
                    format!("Could not save last good item list: {}", e),
                    &LoggerOptions::default(),
                );
            }
            body
        }
        Err(fetch_error) => {
            warning(
                "GameData:Load",
                format!("Fetching {} failed ({}); using last good copy", url, fetch_error),
                &LoggerOptions::default(),
            );
            std::fs::read_to_string(&last_good).map_err(|e| {
                Error::new(
                    "GameData:Load",
                    format!("Item list fetch failed ({}) and no last good copy exists: {}", fetch_error, e),
                    get_location!(),
                )
            })?
        }
    };

    let items: Vec<CacheTradableItem> =
        parse_items_response(&body)?.iter().filter_map(to_tradable_item).collect();
    info("GameData:Load", format!("Loaded {} tradable items", items.len()), &LoggerOptions::default());
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/wfm_items.json");

    fn items() -> Vec<CacheTradableItem> {
        parse_items_response(FIXTURE)
            .unwrap()
            .iter()
            .filter_map(to_tradable_item)
            .collect()
    }

    fn by_slug(slug: &str) -> CacheTradableItem {
        items().into_iter().find(|i| i.wfm_url == slug).unwrap()
    }

    #[test]
    fn maps_identity_fields() {
        let item = by_slug("arcane_energize");
        assert_eq!(item.name, "Arcane Energize");
        assert_eq!(
            item.unique_name,
            "/Lotus/Upgrades/CosmeticEnhancers/Utility/GolemArcaneRadialEnergyOnEnergyPickup"
        );
        assert!(!item.wfm_id.is_empty());
        assert_eq!(item.trade_tax, 0);
        assert_eq!(item.sub_type.as_ref().unwrap().max_rank, Some(5));
    }

    #[test]
    fn maps_variants_and_stars() {
        let relic = by_slug("axi_a1_relic");
        assert!(relic.sub_type.as_ref().unwrap().has_variant("intact"));
        let ayatan = by_slug("ayatan_anasa_sculpture");
        assert!(ayatan.sub_type.as_ref().unwrap().amber_stars.is_some());
    }

    #[test]
    fn items_without_ranks_or_variants_have_no_sub_type() {
        assert!(by_slug("mesa_prime_set").sub_type.is_none());
    }

    #[tokio::test]
    async fn load_items_falls_back_to_last_good_copy() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(LAST_GOOD_FILE), FIXTURE).unwrap();
        // Port 9 (discard) on localhost refuses the connection, so the fetch fails fast.
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(500))
            .build()
            .unwrap();
        let loaded = load_items_from(dir.path(), &http, "http://127.0.0.1:9/v2/items").await.unwrap();
        assert_eq!(loaded.len(), 5);
    }

    #[tokio::test]
    async fn load_items_errors_without_fetch_or_last_good_copy() {
        let dir = tempfile::tempdir().unwrap();
        let http = reqwest::Client::new();
        assert!(load_items_from(dir.path(), &http, "http://127.0.0.1:9/v2/items").await.is_err());
    }
}
