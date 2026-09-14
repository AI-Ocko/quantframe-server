use crate::{
    app::{AppState, CustomSound},
    helper,
};
use std::{
    fs, io,
    path::{Component, Path},
    sync::Mutex,
};
use utils::Error;

const MAX_SOUND_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024;
const ALLOWED_SOUND_EXTENSIONS: [&str; 3] = ["mp3", "wav", "ogg"];

fn normalize_sound_name(name: &str) -> Result<String, Error> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Error::new(
            "Sound",
            "Sound name is required.",
            utils::get_location!(),
        ));
    }
    if trimmed.len() > 100 {
        return Err(Error::new(
            "Sound",
            "Sound name is too long (max 100 characters).",
            utils::get_location!(),
        ));
    }
    if trimmed.chars().any(|ch| ch.is_control()) {
        return Err(Error::new(
            "Sound",
            "Sound name contains invalid characters.",
            utils::get_location!(),
        ));
    }
    Ok(trimmed.to_string())
}

fn validate_sound_file(file_path: &str) -> Result<String, Error> {
    let path = Path::new(file_path);
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .filter(|ext| ALLOWED_SOUND_EXTENSIONS.contains(&ext.as_str()))
        .ok_or_else(|| {
            Error::new(
                "Sound",
                "Unsupported sound file type. Allowed: mp3, wav, ogg.",
                utils::get_location!(),
            )
        })?;

    let metadata = fs::metadata(path).map_err(|e| {
        Error::new(
            "Sound",
            &format!("Failed to read sound file metadata: {}", e),
            utils::get_location!(),
        )
    })?;
    if !metadata.is_file() {
        return Err(Error::new(
            "Sound",
            "Sound file path is not a file.",
            utils::get_location!(),
        ));
    }
    if metadata.len() > MAX_SOUND_FILE_SIZE_BYTES {
        return Err(Error::new(
            "Sound",
            "Sound file is too large. Max size is 10 MB.",
            utils::get_location!(),
        ));
    }

    Ok(extension)
}

fn validate_file_name(file_name: &str) -> Result<(), Error> {
    if file_name.trim().is_empty() {
        return Err(Error::new(
            "Sound",
            "Sound file name is required.",
            utils::get_location!(),
        ));
    }
    let path = Path::new(file_name);
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(Error::new(
            "Sound",
            "Invalid sound file name.",
            utils::get_location!(),
        )),
    }
}

pub async fn sound_get_custom_sounds() -> Result<Vec<CustomSound>, Error> {
    let app = crate::utils::modules::states::app_mutex();
    let app = app.lock()?;
    Ok(app.settings.notifications.custom_sounds.clone())
}

pub async fn sound_add_custom_sound(
    name: String,
    file_name: String,
    data_base64: String,
) -> Result<Vec<CustomSound>, Error> {
    use base64::Engine;
    let app = crate::utils::modules::states::app_mutex();
    let mut app = app.lock()?;

    let normalized_name = normalize_sound_name(&name)?;
    let normalized_name_key = normalized_name.to_lowercase();
    if app
        .settings
        .notifications
        .custom_sounds
        .iter()
        .any(|sound| sound.name_key == normalized_name_key)
    {
        return Err(Error::new("Sound", "Sound name already exists.", utils::get_location!()));
    }
    let extension = file_name
        .rsplit('.')
        .next()
        .map(|e| e.to_lowercase())
        .filter(|e| ALLOWED_SOUND_EXTENSIONS.contains(&e.as_str()))
        .ok_or_else(|| Error::new("Sound", "Only mp3, wav and ogg files are allowed.", utils::get_location!()))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|e| Error::new("Sound", format!("Invalid file data: {}", e), utils::get_location!()))?;
    if bytes.len() as u64 > MAX_SOUND_FILE_SIZE_BYTES {
        return Err(Error::new("Sound", "Sound file is too large (max 10 MB).", utils::get_location!()));
    }

    let stored_name = format!("{}.{}", uuid::Uuid::new_v4(), extension);
    fs::write(helper::get_sounds_path().join(&stored_name), bytes).map_err(|e| {
        Error::new("Sound", format!("Failed to save sound file: {}", e), utils::get_location!())
    })?;

    app.settings
        .notifications
        .custom_sounds
        .push(CustomSound::new(normalized_name, stored_name));
    app.settings.save()?;
    Ok(app.settings.notifications.custom_sounds.clone())
}

pub async fn sound_delete_custom_sound(
    file_name: String) -> Result<Vec<CustomSound>, Error> {
    let app = crate::utils::modules::states::app_mutex();
    let mut app = app.lock()?;

    validate_file_name(&file_name)?;

    // Remove file from sounds dir
    let sounds_path = helper::get_sounds_path();
    let file_path = sounds_path.join(&file_name);

    if let Err(error) = fs::remove_file(&file_path) {
        if error.kind() != io::ErrorKind::NotFound {
            return Err(Error::new(
                "Sound",
                &format!("Failed to delete sound file: {}", error),
                utils::get_location!(),
            ));
        }
    }

    // Remove from settings
    app.settings
        .notifications
        .custom_sounds
        .retain(|s| s.file_name != file_name);
    app.settings.save()?;

    Ok(app.settings.notifications.custom_sounds.clone())
}

