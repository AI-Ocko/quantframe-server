use std::{fs::File, io::Read, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utils::{get_location, validate_json, Error};

use crate::helper;

fn get_path() -> PathBuf {
    helper::get_app_storage_path().join("auth.json")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub anonymous: bool,
    pub verification: bool,
    pub wfm_banned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wfm_banned_until: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wfm_banned_reason: Option<String>,
    pub wfm_id: String,
    #[serde(skip)]
    pub wfm_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wfm_avatar: Option<String>,
    pub wfm_username: String,
    pub locale: String,
    pub platform: String,
    pub wfm_status: String,
}
impl Default for User {
    fn default() -> Self {
        User {
            wfm_token: String::new(),
            wfm_id: String::new(),
            wfm_username: String::new(),
            anonymous: true,
            verification: false,
            wfm_banned: false,
            wfm_status: String::from("invisible"),
            wfm_banned_until: None,
            wfm_banned_reason: None,
            wfm_avatar: None,
            locale: "en".to_string(),
            platform: String::new(),
        }
    }
}

impl User {
    pub fn load() -> Result<Self, Error> {
        let path = &get_path();
        if !path.exists() {
            let user = User::default();
            user.save()?;
            return Ok(user);
        }
        // Open the file and read its contents
        let mut file = File::open(path).map_err(|e| {
            Error::from_io("User", path, "Failed to open auth file", e, get_location!())
        })?;
        let mut content = String::new();
        file.read_to_string(&mut content).map_err(|e| {
            Error::from_io("User", path, "Failed to read auth file", e, get_location!())
        })?;

        // Parse the JSON string into a Value object
        let json_value: Value = serde_json::from_str(&content).map_err(|e| {
            Error::from_json(
                "User",
                path,
                &content,
                "Failed to parse auth file",
                e,
                get_location!(),
            )
        })?;

        // Required properties for the settings.json file
        let required_json = serde_json::to_value(User::default()).map_err(|e| {
            Error::from_json(
                "User",
                path,
                &content,
                "Failed to serialize default user",
                e,
                get_location!(),
            )
        })?;

        // Validate the JSON object against the required properties
        let (validated_json, missing_properties) = validate_json(&json_value, &required_json, "");

        // Check for missing properties
        if !missing_properties.is_empty() {
            for property in missing_properties.clone() {
                println!("Missing property: {}", property);
            }
        }
        match serde_json::from_value::<User>(validated_json) {
            Ok(user) => Ok(user),
            Err(_) => {
                let default_user = User::default();
                default_user.save()?;
                return Ok(default_user);
            }
        }
    }
    pub fn save(&self) -> Result<(), Error> {
        let path = &get_path();
        // Format the user data as a JSON string
        let content = serde_json::to_string(self).map_err(|e| {
            Error::from_json(
                "User",
                path,
                "N/A",
                "Failed to serialize user",
                e,
                get_location!(),
            )
        })?;
        std::fs::write(path, content).map_err(|e| {
            Error::from_io(
                "User",
                path,
                "Failed to write user to file",
                e,
                get_location!(),
            )
        })?;
        Ok(())
    }

    pub fn is_banned(&self) -> bool {
        self.wfm_banned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_never_serialized() {
        let mut user = User::default();
        user.wfm_token = "secret".into();
        let json = serde_json::to_string(&user).unwrap();
        assert!(!json.contains("secret"));
        assert!(!json.contains("wfm_token"));
    }
}
