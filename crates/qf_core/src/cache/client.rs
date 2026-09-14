use std::{path::PathBuf, sync::Arc};

use utils::Error;

use super::modules::{ThemeModule, TradableItemModule};
use super::types::CacheTradableItem;

#[derive(Clone, Debug)]
pub struct CacheState {
    pub base_path: PathBuf,
    tradable_item_module: Arc<TradableItemModule>,
    theme_module: Arc<ThemeModule>,
}

impl CacheState {
    pub fn new(base_path: PathBuf) -> Self {
        let theme_module = ThemeModule::new(&base_path);
        Self { base_path, tradable_item_module: TradableItemModule::new(), theme_module }
    }

    pub fn load(&self, items: Vec<CacheTradableItem>) -> Result<(), Error> {
        self.tradable_item_module.set_items(items);
        self.theme_module.load()
    }

    pub fn tradable_item(&self) -> Arc<TradableItemModule> {
        self.tradable_item_module.clone()
    }

    pub fn theme(&self) -> Arc<ThemeModule> {
        self.theme_module.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tradable_items_are_found_by_every_key() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CacheState::new(dir.path().to_path_buf());
        let items = crate::game_data::parse_items_response(include_str!("../../tests/fixtures/wfm_items.json"))
            .unwrap()
            .iter()
            .filter_map(crate::game_data::to_tradable_item)
            .collect::<Vec<_>>();
        let energize = items.iter().find(|i| i.wfm_url == "arcane_energize").unwrap().clone();
        cache.load(items).unwrap();
        let module = cache.tradable_item();
        assert_eq!(module.get_by("arcane_energize").unwrap().wfm_id, energize.wfm_id);
        assert_eq!(module.get_by(&energize.wfm_id).unwrap().wfm_url, "arcane_energize");
        assert_eq!(module.get_by("Arcane Energize").unwrap().wfm_url, "arcane_energize");
        assert_eq!(module.get_by(&energize.unique_name).unwrap().wfm_url, "arcane_energize");
        assert!(module.get_by("does_not_exist").is_err());
        assert_eq!(module.get_items().unwrap().len(), 5);
    }
}
