//! Folds a full set of parts into the set item, with WFM `setParts` fetched lazily (amendment E7).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use utils::{warning, LoggerOptions};

use super::resolve::ItemIndex;
use super::ResolvedItem;
use crate::market::limiter::{self, Lane};

pub const SETS_FILE: &str = "sets.json";
pub const WFM_ITEM_URL: &str = "https://api.warframe.market/v2/item";

/// Part slugs per set-root slug, without the root itself.
pub type PartsMap = HashMap<String, Vec<String>>;

#[async_trait]
pub trait SetSource: Send + Sync {
    /// Parts for the given roots. Roots it can't provide are absent from the result.
    async fn parts_for(&self, roots: &[String], index: &ItemIndex) -> PartsMap;
}

#[async_trait]
impl SetSource for PartsMap {
    async fn parts_for(&self, roots: &[String], _index: &ItemIndex) -> PartsMap {
        roots.iter().filter_map(|root| self.get(root).map(|parts| (root.clone(), parts.clone()))).collect()
    }
}

/// Set roots whose English name without ` Set` prefixes at least two of the items' names.
pub fn set_candidates(items: &[ResolvedItem], index: &ItemIndex) -> Vec<String> {
    if items.len() < 2 {
        return Vec::new();
    }
    let names: Vec<String> = items.iter().map(|i| i.item_name.to_lowercase()).collect();
    let mut roots: Vec<String> = index
        .set_roots()
        .filter_map(|root| {
            let prefix = format!("{} ", root.name.strip_suffix(" Set")?.to_lowercase());
            (names.iter().filter(|n| n.starts_with(&prefix)).count() >= 2).then(|| root.wfm_url.clone())
        })
        .collect();
    roots.sort();
    roots
}

/// Replaces every complete set of parts with the set item; leftover parts stay.
pub fn fold_sets(mut items: Vec<ResolvedItem>, candidates: &[String], parts: &PartsMap, index: &ItemIndex) -> Vec<ResolvedItem> {
    for root in candidates {
        let (Some(part_slugs), Some(root_item)) = (parts.get(root), index.by_slug(root)) else { continue };
        let part_slugs: Vec<&String> = part_slugs.iter().filter(|p| *p != root).collect();
        if part_slugs.is_empty() {
            continue;
        }
        let sets = part_slugs
            .iter()
            .map(|slug| items.iter().filter(|i| &i.slug == *slug).map(|i| i.quantity).sum::<i64>())
            .min()
            .unwrap_or(0);
        if sets <= 0 {
            continue;
        }
        for slug in &part_slugs {
            let mut remaining = sets;
            for item in items.iter_mut().filter(|i| &i.slug == *slug) {
                let take = remaining.min(item.quantity);
                item.quantity -= take;
                remaining -= take;
            }
        }
        items.retain(|i| i.quantity > 0);
        items.push(ResolvedItem {
            name: root_item.name.clone(),
            slug: root_item.wfm_url.clone(),
            wfm_id: root_item.wfm_id.clone(),
            item_name: root_item.name.clone(),
            sub_type: None,
            quantity: sets,
            price: 0,
            matched_by: "set".into(),
        });
    }
    items
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemDetail {
    #[serde(default)]
    set_parts: Vec<String>,
}

#[derive(Deserialize)]
struct ItemDetailResponse {
    data: ItemDetail,
}

/// `GET /v2/item/{slug}` lists `setParts` as item ids. An id the index doesn't know makes the whole
/// list an error: dropping it would leave a short list that folds an incomplete set as complete.
pub fn parse_set_parts(json: &str, index: &ItemIndex) -> Result<Vec<String>, String> {
    let detail: ItemDetailResponse = serde_json::from_str(json).map_err(|e| e.to_string())?;
    detail
        .data
        .set_parts
        .iter()
        .map(|id| index.by_id(id).map(|i| i.wfm_url.clone()).ok_or_else(|| format!("unknown item id {id}")))
        .collect()
}

/// Memory, then `QF_DATA_DIR/cache/sets.json`, then one WFM fetch per unknown root.
pub struct SetCache {
    file: PathBuf,
    parts: Mutex<PartsMap>,
    http: reqwest::Client,
}

impl SetCache {
    pub fn new(cache_dir: &Path) -> Self {
        let file = cache_dir.join(SETS_FILE);
        let parts = std::fs::read_to_string(&file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().expect("HTTP client");
        Self { file, parts: Mutex::new(parts), http }
    }

    fn save(&self) {
        let snapshot = self.parts.lock().unwrap().clone();
        match serde_json::to_string(&snapshot) {
            Ok(text) => {
                // Write beside the file and rename over it, like Queue::pop, so a crash never leaves a truncated sets.json.
                let mut temp = self.file.clone().into_os_string();
                temp.push(".tmp");
                let temp = std::path::PathBuf::from(temp);
                if let Err(e) = std::fs::write(&temp, text).and_then(|_| std::fs::rename(&temp, &self.file)) {
                    warning("HelperLink:Sets", format!("Could not save {}: {e}", self.file.display()), &LoggerOptions::default());
                }
            }
            Err(e) => warning("HelperLink:Sets", format!("Could not serialise sets: {e}"), &LoggerOptions::default()),
        }
    }

    /// Caches a freshly fetched list unless it is empty. An empty list would disable folding for
    /// that root forever, so it is dropped and the next trade retries the fetch. `true` when stored.
    fn remember(&self, root: &str, parts: Vec<String>) -> bool {
        if parts.is_empty() {
            warning("HelperLink:Sets", format!("WFM listed no set parts for {root}; not cached"), &LoggerOptions::default());
            return false;
        }
        self.parts.lock().unwrap().insert(root.to_string(), parts);
        true
    }

    async fn fetch(&self, root: &str, index: &ItemIndex) -> Result<Vec<String>, String> {
        limiter::global().acquire(Lane::Hot).await;
        let response = self
            .http
            .get(format!("{WFM_ITEM_URL}/{root}"))
            .header("Language", "en")
            .header("Platform", "pc")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if response.status().as_u16() == 429 {
            limiter::global().report_429();
        }
        let body = response.error_for_status().map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())?;
        parse_set_parts(&body, index)
    }
}

#[async_trait]
impl SetSource for SetCache {
    async fn parts_for(&self, roots: &[String], index: &ItemIndex) -> PartsMap {
        let mut fetched_any = false;
        for root in roots {
            if self.parts.lock().unwrap().contains_key(root) {
                continue;
            }
            match self.fetch(root, index).await {
                Ok(parts) => fetched_any |= self.remember(root, parts),
                Err(e) => warning("HelperLink:Sets", format!("Could not fetch set parts for {root}: {e}"), &LoggerOptions::default()),
            }
        }
        if fetched_any {
            self.save();
        }
        let parts = self.parts.lock().unwrap();
        roots.iter().filter_map(|root| parts.get(root).map(|p| (root.clone(), p.clone()))).collect()
    }
}

static SET_CACHE: OnceLock<SetCache> = OnceLock::new();

pub fn cache() -> &'static SetCache {
    SET_CACHE.get_or_init(|| SetCache::new(&crate::paths::get().cache_dir()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_link::trades::resolve::tests::index;

    fn part(slug: &str, quantity: i64) -> ResolvedItem {
        let index = index();
        let item = index.by_slug(slug).unwrap();
        ResolvedItem {
            name: item.name.clone(),
            slug: slug.into(),
            wfm_id: item.wfm_id.clone(),
            item_name: item.name.clone(),
            sub_type: None,
            quantity,
            price: 0,
            matched_by: "name".into(),
        }
    }

    fn wolf_parts() -> PartsMap {
        PartsMap::from([(
            "wolf_sledge_set".to_string(),
            vec!["wolf_sledge_blueprint".into(), "wolf_sledge_motor".into(), "wolf_sledge_head".into(), "wolf_sledge_handle".into()],
        )])
    }

    #[test]
    fn candidates_need_two_items_sharing_the_root_prefix() {
        let index = index();
        assert!(set_candidates(&[part("wolf_sledge_handle", 1)], &index).is_empty());
        assert_eq!(set_candidates(&[part("wolf_sledge_handle", 1), part("wolf_sledge_head", 1)], &index), vec!["wolf_sledge_set".to_string()]);
        assert!(set_candidates(&[part("wolf_sledge_handle", 1), part("adaptation", 1)], &index).is_empty());
    }

    #[test]
    fn a_full_set_folds_and_leftovers_stay() {
        let index = index();
        let items = vec![part("wolf_sledge_blueprint", 1), part("wolf_sledge_motor", 2), part("wolf_sledge_head", 1), part("wolf_sledge_handle", 1)];
        let folded = fold_sets(items, &["wolf_sledge_set".into()], &wolf_parts(), &index);
        let mut slugs: Vec<(String, i64, String)> = folded.iter().map(|i| (i.slug.clone(), i.quantity, i.matched_by.clone())).collect();
        slugs.sort();
        assert_eq!(slugs, vec![("wolf_sledge_motor".into(), 1, "name".into()), ("wolf_sledge_set".into(), 1, "set".into())]);
    }

    #[test]
    fn two_sets_fold_into_quantity_two() {
        let index = index();
        let items = vec![part("wolf_sledge_blueprint", 2), part("wolf_sledge_motor", 2), part("wolf_sledge_head", 2), part("wolf_sledge_handle", 2)];
        let folded = fold_sets(items, &["wolf_sledge_set".into()], &wolf_parts(), &index);
        assert_eq!(folded.len(), 1);
        assert_eq!((folded[0].slug.as_str(), folded[0].quantity), ("wolf_sledge_set", 2));
    }

    #[test]
    fn an_incomplete_set_or_unknown_parts_change_nothing() {
        let index = index();
        let items = vec![part("wolf_sledge_blueprint", 1), part("wolf_sledge_motor", 1)];
        assert_eq!(fold_sets(items.clone(), &["wolf_sledge_set".into()], &wolf_parts(), &index), items);
        assert_eq!(fold_sets(items.clone(), &["wolf_sledge_set".into()], &PartsMap::new(), &index), items);
    }

    #[test]
    fn set_parts_parse_from_the_wfm_item_response() {
        let index = index();
        let json = r#"{"data":{"slug":"wolf_sledge_set","setRoot":true,"setParts":["id_wolf_sledge_handle","id_wolf_sledge_set"]}}"#;
        assert_eq!(parse_set_parts(json, &index).unwrap(), vec!["wolf_sledge_handle".to_string(), "wolf_sledge_set".to_string()]);
        let unknown = r#"{"data":{"slug":"wolf_sledge_set","setParts":["id_wolf_sledge_handle","nope"]}}"#;
        assert_eq!(parse_set_parts(unknown, &index).unwrap_err(), "unknown item id nope", "a short list would fold an incomplete set");
        assert!(parse_set_parts("nope", &index).is_err());
        assert_eq!(parse_set_parts(r#"{"data":{"slug":"x"}}"#, &index).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn an_empty_parts_list_is_neither_cached_nor_saved() {
        let dir = tempfile::tempdir().unwrap();
        let cache = SetCache::new(dir.path());
        assert!(!cache.remember("wolf_sledge_set", Vec::new()), "an empty list is dropped");
        assert!(cache.remember("mesa_prime_set", vec!["mesa_prime_blueprint".into()]));
        assert!(!cache.parts.lock().unwrap().contains_key("wolf_sledge_set"));
        cache.save();
        let on_disk: PartsMap = serde_json::from_str(&std::fs::read_to_string(dir.path().join(SETS_FILE)).unwrap()).unwrap();
        assert!(!on_disk.contains_key("wolf_sledge_set"), "the empty root never reaches sets.json");
        assert!(on_disk.contains_key("mesa_prime_set"));
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let cache = SetCache::new(dir.path());
        cache.parts.lock().unwrap().insert("wolf_sledge_set".into(), vec!["wolf_sledge_handle".into()]);
        cache.save();
        cache.save();
        assert!(dir.path().join(SETS_FILE).is_file());
        assert!(!dir.path().join(format!("{SETS_FILE}.tmp")).exists());
        let reread = SetCache::new(dir.path());
        assert_eq!(reread.parts.lock().unwrap().get("wolf_sledge_set").map(Vec::len), Some(1));
    }

    #[tokio::test]
    async fn the_cache_serves_from_disk_without_fetching() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(SETS_FILE), serde_json::to_string(&wolf_parts()).unwrap()).unwrap();
        let cache = SetCache::new(dir.path());
        let parts = cache.parts_for(&["wolf_sledge_set".into()], &index()).await;
        assert_eq!(parts["wolf_sledge_set"].len(), 4);
    }
}
