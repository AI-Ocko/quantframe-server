use std::{fs, path::PathBuf, sync::OnceLock};

use utils::{get_location, Error};

#[derive(Debug, Clone)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub resources_dir: PathBuf,
    pub backup_dir: PathBuf,
}

static PATHS: OnceLock<Paths> = OnceLock::new();

impl Paths {
    pub fn new(
        data_dir: impl Into<PathBuf>,
        resources_dir: impl Into<PathBuf>,
        backup_dir: Option<PathBuf>,
    ) -> Result<Self, Error> {
        let data_dir = data_dir.into();
        fs::create_dir_all(&data_dir).map_err(|e| {
            Error::new(
                "Paths:New",
                format!("Failed to create data dir {}: {}", data_dir.display(), e),
                get_location!(),
            )
        })?;
        let backup_dir = backup_dir.unwrap_or_else(|| data_dir.join("backups"));
        Ok(Self { data_dir, resources_dir: resources_dir.into(), backup_dir })
    }

    fn subdir(&self, name: &str) -> PathBuf {
        let path = self.data_dir.join(name);
        let _ = fs::create_dir_all(&path);
        path
    }

    pub fn sounds_dir(&self) -> PathBuf {
        self.subdir("sounds")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.subdir("cache")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.subdir("logs")
    }

    /// `QF_BACKUP_DIR`, default `<data_dir>/backups`; created on demand (spec §19 H6).
    pub fn backups_dir(&self) -> PathBuf {
        let _ = fs::create_dir_all(&self.backup_dir);
        self.backup_dir.clone()
    }

    /// Stable per-installation id, generated once and stored in `<data_dir>/device_id`.
    pub fn device_id(&self) -> Result<String, Error> {
        let file = self.data_dir.join("device_id");
        if let Ok(existing) = fs::read_to_string(&file) {
            let existing = existing.trim().to_string();
            if !existing.is_empty() {
                return Ok(existing);
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        fs::write(&file, &id).map_err(|e| {
            Error::new("Paths:DeviceId", format!("Failed to write device id: {}", e), get_location!())
        })?;
        Ok(id)
    }
}

pub fn init(paths: Paths) {
    let _ = PATHS.set(paths);
}

pub fn get() -> &'static Paths {
    PATHS.get().expect("Paths not initialized")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_is_created_once_and_reused() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path(), dir.path().join("res"), None).unwrap();
        let first = paths.device_id().unwrap();
        let second = paths.device_id().unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 36);
        assert!(paths.sounds_dir().is_dir());
        assert_eq!(paths.backups_dir(), dir.path().join("backups"));

        let elsewhere = Paths::new(dir.path(), dir.path().join("res"), Some(dir.path().join("elsewhere"))).unwrap();
        assert_eq!(elsewhere.backups_dir(), dir.path().join("elsewhere"));
        assert!(elsewhere.backups_dir().is_dir());
    }
}
