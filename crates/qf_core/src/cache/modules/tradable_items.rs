use std::sync::{Arc, Mutex};

use utils::{get_location, Error, MultiKeyMap};

use crate::cache::types::CacheTradableItem;

#[derive(Debug)]
pub struct TradableItemModule {
    items: Mutex<Vec<CacheTradableItem>>,
    item_lookup: Mutex<MultiKeyMap<CacheTradableItem>>,
}

impl TradableItemModule {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            items: Mutex::new(Vec::new()),
            item_lookup: Mutex::new(MultiKeyMap::new()),
        })
    }

    pub fn set_items(&self, items: Vec<CacheTradableItem>) {
        let mut lookup = MultiKeyMap::new();
        for item in items.iter() {
            let mut keys = vec![item.wfm_id.clone(), item.name.clone(), item.wfm_url.clone()];
            if !item.unique_name.is_empty() {
                keys.push(item.unique_name.clone());
            }
            keys.extend(item.variant_to_unique_name.values().cloned());
            lookup.insert_value(item.clone(), keys);
        }
        *self.item_lookup.lock().unwrap() = lookup;
        *self.items.lock().unwrap() = items;
    }
    pub fn get_items(&self) -> Result<Vec<CacheTradableItem>, Error> {
        let items = self
            .items
            .lock()
            .expect("Failed to lock items mutex")
            .clone();
        Ok(items)
    }
    /* -------------------------------------------------------------
        Lookup Functions
    ------------------------------------------------------------- */
    /// Get a tradable item by various identifiers
    ///  # Arguments
    /// - `item_id`: The identifier to search for (name, url, unique name, or id)
    ///
    pub fn get_by(&self, item_id: impl Into<String>) -> Result<CacheTradableItem, Error> {
        let item_id: String = item_id.into();
        let item_lookup = self.item_lookup.lock().unwrap();
        if let Some(item) = item_lookup.get(&item_id) {
            Ok(item.clone())
        } else {
            Err(Error::new(
                "Cache:TradableItem:GetBy",
                format!("Tradable item not found for id '{}'", item_id),
                get_location!(),
            ))
        }
    }
}
