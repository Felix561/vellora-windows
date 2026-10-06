use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::Context;

use crate::{brand, types::AppSettings};

const MAX_SETTINGS_BYTES: u64 = 64 * 1024;

pub fn app_data_dir() -> anyhow::Result<PathBuf> {
    let base = dirs::data_dir().context("Could not find Windows app data directory")?;
    let dir = base.join(brand::DATA_DIR_NAME);
    fs::create_dir_all(&dir).context("Could not create Vellora app data directory")?;
    Ok(dir)
}

pub fn legacy_app_data_dir() -> anyhow::Result<PathBuf> {
    let base = dirs::data_dir().context("Could not find Windows app data directory")?;
    Ok(base.join(brand::OLD_DATA_DIR_NAME))
}

pub fn settings_path() -> anyhow::Result<PathBuf> {
    Ok(app_data_dir()?.join("settings.json"))
}

pub fn legacy_settings_path() -> anyhow::Result<PathBuf> {
    Ok(legacy_app_data_dir()?.join("settings.json"))
}

pub fn load_settings_with_warning() -> anyhow::Result<(AppSettings, Option<String>)> {
    let path = settings_path()?;
    load_settings_from(&path)
}

fn load_settings_from(path: &Path) -> anyhow::Result<(AppSettings, Option<String>)> {
    if !path.exists() {
        return Ok((AppSettings::default(), None));
    }

    let mut contents = Vec::new();
    fs::File::open(path)
        .context("Could not open settings file")?
        .take(MAX_SETTINGS_BYTES + 1)
        .read_to_end(&mut contents)
        .context("Could not read settings file")?;
    let parsed = if contents.len() as u64 > MAX_SETTINGS_BYTES {
        Err(anyhow::anyhow!("Settings file exceeds the supported size"))
    } else {
        serde_json::from_slice::<AppSettings>(&contents)
            .context("Could not parse settings file")
            .and_then(|settings| {
                validate_settings(&settings)?;
                Ok(settings)
            })
    };
    match parsed {
        Ok(settings) => Ok((settings, None)),
        Err(_) => {
            let backup =
                path.with_file_name(format!("settings-invalid-{}.json", uuid::Uuid::new_v4()));
            // Preserve the original before returning defaults. Never silently
            // overwrite settings that this version cannot understand.
            fs::rename(path, backup).context("Could not preserve invalid settings file")?;
            Ok((
                AppSettings::default(),
                Some("Vellora could not read your saved settings. Defaults were restored and the original file was preserved as settings-invalid-*.json in the app data folder.".to_string()),
            ))
        }
    }
}

pub fn save_settings(settings: &AppSettings) -> anyhow::Result<()> {
    let path = settings_path()?;
    save_settings_to(&path, settings)
}

pub fn validate_settings(settings: &AppSettings) -> anyhow::Result<()> {
    anyhow::ensure!(
        settings.language == "auto"
            || (settings.language.len() == 2
                && settings
                    .language
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase())),
        "Language must be auto or a two-letter lowercase language code"
    );
    anyhow::ensure!(
        !settings.cleanup_model.is_empty()
            && settings.cleanup_model.len() <= 128
            && settings.cleanup_model.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            }),
        "Cleanup model must be a valid OpenAI model ID"
    );
    Ok(())
}

fn save_settings_to(path: &Path, settings: &AppSettings) -> anyhow::Result<()> {
    validate_settings(settings)?;
    let contents =
        serde_json::to_string_pretty(settings).context("Could not serialize settings")?;
    let parent = path
        .parent()
        .context("Settings path has no parent directory")?;
    let mut pending = tempfile::Builder::new()
        .prefix("settings-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .context("Could not create settings staging file")?;
    pending
        .write_all(contents.as_bytes())
        .context("Could not write settings staging file")?;
    pending
        .as_file()
        .sync_all()
        .context("Could not flush settings staging file")?;
    pending
        .persist(path)
        .context("Could not replace settings file")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_save_replaces_existing_file_without_leaving_staging_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        save_settings_to(&path, &AppSettings::default()).unwrap();
        let settings = AppSettings {
            language: "de".to_string(),
            ..AppSettings::default()
        };
        save_settings_to(&path, &settings).unwrap();
        let (loaded, warning) = load_settings_from(&path).unwrap();
        assert_eq!(loaded.language, "de");
        assert!(warning.is_none());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn invalid_settings_are_preserved_and_defaults_can_be_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{ truncated").unwrap();
        let (settings, warning) = load_settings_from(&path).unwrap();
        assert!(warning.is_some());
        assert_eq!(settings.language, AppSettings::default().language);
        let backup = fs::read_dir(dir.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(fs::read_to_string(backup).unwrap(), "{ truncated");
        save_settings_to(&path, &settings).unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn failed_validation_does_not_replace_existing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        save_settings_to(&path, &AppSettings::default()).unwrap();
        let before = fs::read(&path).unwrap();
        let settings = AppSettings {
            language: "not-a-language".to_string(),
            ..AppSettings::default()
        };
        assert!(save_settings_to(&path, &settings).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn failed_replace_removes_staging_file() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("settings.json");
        fs::create_dir(&destination).unwrap();
        assert!(save_settings_to(&destination, &AppSettings::default()).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn oversized_and_non_utf8_settings_are_preserved_intact() {
        for contents in [
            vec![b'x'; MAX_SETTINGS_BYTES as usize + 20],
            vec![255, 254, 0],
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("settings.json");
            fs::write(&path, &contents).unwrap();
            let (_, warning) = load_settings_from(&path).unwrap();
            assert!(warning.is_some());
            let backup = fs::read_dir(dir.path())
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            assert_eq!(fs::read(backup).unwrap(), contents);
        }
    }
}
