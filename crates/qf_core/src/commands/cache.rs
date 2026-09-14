use utils::Error;

use crate::cache::types::*;

pub async fn cache_get_tradable_items() -> Result<Vec<CacheTradableItem>, Error> {
    let cache = crate::utils::modules::states::cache_mutex();
    let cache = cache.lock()?;
    match cache.tradable_item().get_items() {
        Ok(items) => {
            return Ok(items);
        }
        Err(e) => {
            e.log("cache_get_tradable_items.log");
            return Err(e);
        }
    }
}
pub async fn cache_get_theme_presets() -> Result<Vec<CacheTheme>, Error> {
    let cache = crate::utils::modules::states::cache_mutex();
    let cache = cache.lock()?;
    match cache.theme().get_items() {
        Ok(items) => {
            return Ok(items);
        }
        Err(e) => {
            e.log("cache_get_theme_presets.log");
            return Err(e);
        }
    }
}
