use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use utils::{get_location, info, read_json_file_optional, Error, LoggerOptions};

use crate::cache::types::CacheTheme;

#[derive(Debug)]
pub struct ThemeModule {
    path: PathBuf,
    items: Mutex<Vec<CacheTheme>>,
}
impl ThemeModule {
    pub fn new(base_path: &Path) -> Arc<Self> {
        Arc::new(Self {
            path: base_path.join("themePresets"),
            items: Mutex::new(Vec::new()),
        })
    }
    pub fn get_items(&self) -> Result<Vec<CacheTheme>, Error> {
        let items = self
            .items
            .lock()
            .expect("Failed to lock items mutex")
            .clone();
        Ok(items)
    }
    pub fn get_theme_folder(&self) -> PathBuf {
        self.path.clone()
    }
    pub fn load(&self) -> Result<(), Error> {
        if !self.path.exists() {
            info(
                "Cache:Theme:load",
                "Theme cache path does not exist, creating it.",
                &LoggerOptions::default(),
            );
            std::fs::create_dir_all(&self.path).map_err(|e| {
                Error::from(e)
                    .with_location(get_location!())
                    .set_message("Failed to create theme cache directory")
            })?;
        }

        if !self.path.is_dir() {
            return Err(Error::new(
                "Cache:Theme:load",
                "Theme cache path exists but is not a directory.",
                get_location!(),
            ));
        }

        // Get All files in the path
        let files = match std::fs::read_dir(&self.path) {
            Ok(files) => files,
            Err(e) => {
                return Err(Error::from(e).with_location(get_location!()));
            }
        };
        let mut items_lock = self.items.lock().unwrap();
        items_lock.clear(); // Clear existing items before loading new ones
        for file in files {
            let file = file.map_err(|e| Error::from(e).with_location(get_location!()))?;
            match read_json_file_optional::<CacheTheme>(&file.path()) {
                Ok(items) => {
                    items_lock.push(items);
                }
                Err(e) => return Err(e.with_location(get_location!())),
            }
        }
        info(
            "Cache:Theme:load",
            &format!("Loaded {} themes", items_lock.len()),
            &LoggerOptions::default(),
        );

        Ok(())
    }
}
