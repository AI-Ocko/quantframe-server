use std::path::PathBuf;

use qf_core::startup::CoreConfig;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: String,
    pub data_dir: PathBuf,
    pub web_dir: PathBuf,
    pub resources_dir: PathBuf,
    pub public_origin: String,
    pub secret_key_file: PathBuf,
    pub web_password_file: PathBuf,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let or = |key: &str, default: &str| get(key).unwrap_or_else(|| default.to_string());
        let public_origin = get("QF_PUBLIC_ORIGIN")
            .ok_or("QF_PUBLIC_ORIGIN is required, e.g. http://homelab.lan:8080")?;
        Ok(Self {
            bind: or("QF_BIND", "0.0.0.0:8080"),
            data_dir: or("QF_DATA_DIR", "/data").into(),
            web_dir: or("QF_WEB_DIR", "/app/web").into(),
            resources_dir: or("QF_RESOURCES_DIR", "/app/resources").into(),
            public_origin: public_origin.trim_end_matches('/').to_string(),
            secret_key_file: or("QF_SECRET_KEY_FILE", "/run/secrets/qf_secret_key").into(),
            web_password_file: or("QF_WEB_PASSWORD_FILE", "/run/secrets/qf_web_password").into(),
        })
    }

    pub fn core(&self) -> CoreConfig {
        CoreConfig {
            data_dir: self.data_dir.clone(),
            resources_dir: self.resources_dir.clone(),
            secret_key_hex: std::fs::read_to_string(&self.secret_key_file).ok(),
            web_password_file: self.web_password_file.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_is_required_and_trailing_slash_trimmed() {
        assert!(Config::from_lookup(|_| None).is_err());
        let cfg = Config::from_lookup(|k| (k == "QF_PUBLIC_ORIGIN").then(|| "http://h:8080/".to_string())).unwrap();
        assert_eq!(cfg.public_origin, "http://h:8080");
        assert_eq!(cfg.bind, "0.0.0.0:8080");
    }
}
