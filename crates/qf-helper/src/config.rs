use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Warframe's EE.log under Proton (Steam app 230410), relative to `$HOME`.
pub const DEFAULT_EE_LOG: &str =
    ".local/share/Steam/steamapps/compatdata/230410/pfx/drive_c/users/steamuser/AppData/Local/Warframe/EE.log";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    server_url: String,
    device_key: String,
    ee_log_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub server_url: String,
    pub device_key: String,
    /// Read in phase 4b; parsed now so the config file doesn't change.
    pub ee_log_path: PathBuf,
}

impl Config {
    pub fn parse(text: &str, home: &Path) -> Result<Self, String> {
        let raw: RawConfig = toml::from_str(text).map_err(|e| format!("Invalid qf-helper.toml: {e}"))?;
        let server_url = raw.server_url.trim().trim_end_matches('/').to_string();
        if !(server_url.starts_with("http://") || server_url.starts_with("https://")) {
            return Err("server_url must start with http:// or https://".into());
        }
        let device_key = raw.device_key.trim().to_string();
        if !device_key.starts_with("qfh_") {
            return Err("device_key must be a key created in the web UI (it starts with qfh_)".into());
        }
        Ok(Self { server_url, device_key, ee_log_path: raw.ee_log_path.unwrap_or_else(|| home.join(DEFAULT_EE_LOG)) })
    }

    pub fn load(path: &Path, home: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        Self::parse(&text, home)
    }
}

/// `$XDG_CONFIG_HOME/qf-helper/qf-helper.toml`, or `~/.config/qf-helper/qf-helper.toml`.
pub fn default_config_path(xdg_config_home: Option<&str>, home: &Path) -> PathBuf {
    let base = match xdg_config_home {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home.join(".config"),
    };
    base.join("qf-helper").join("qf-helper.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/player";

    #[test]
    fn minimal_config_uses_the_proton_log_path() {
        let config = Config::parse("server_url = \"http://ockohome:8080/\"\ndevice_key = \"qfh_abc\"\n", Path::new(HOME)).unwrap();
        assert_eq!(config.server_url, "http://ockohome:8080");
        assert_eq!(config.device_key, "qfh_abc");
        assert_eq!(config.ee_log_path, Path::new(HOME).join(DEFAULT_EE_LOG));
    }

    #[test]
    fn explicit_log_path_is_kept() {
        let text = "server_url = \"https://qf.lan\"\ndevice_key = \"qfh_abc\"\nee_log_path = \"/games/EE.log\"\n";
        assert_eq!(Config::parse(text, Path::new(HOME)).unwrap().ee_log_path, PathBuf::from("/games/EE.log"));
    }

    #[test]
    fn invalid_configs_are_rejected_with_a_reason() {
        let home = Path::new(HOME);
        assert!(Config::parse("server_url = \"ockohome:8080\"\ndevice_key = \"qfh_abc\"\n", home).unwrap_err().contains("server_url"));
        assert!(Config::parse("server_url = \"http://ockohome:8080\"\ndevice_key = \"abc\"\n", home).unwrap_err().contains("device_key"));
        assert!(Config::parse("server_url = \"http://ockohome:8080\"\n", home).is_err(), "device_key is required");
        assert!(
            Config::parse("server_url = \"http://ockohome:8080\"\ndevice_key = \"qfh_abc\"\ndevice_kye = \"x\"\n", home).is_err(),
            "typos in key names are rejected"
        );
    }

    #[test]
    fn config_path_prefers_xdg_config_home() {
        let home = Path::new(HOME);
        assert_eq!(default_config_path(Some("/cfg"), home), PathBuf::from("/cfg/qf-helper/qf-helper.toml"));
        assert_eq!(default_config_path(Some(""), home), PathBuf::from("/home/player/.config/qf-helper/qf-helper.toml"));
        assert_eq!(default_config_path(None, home), PathBuf::from("/home/player/.config/qf-helper/qf-helper.toml"));
    }
}
