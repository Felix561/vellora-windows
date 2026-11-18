use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::Context;

use crate::{
    brand,
    text_providers::ModelRef,
    types::{
        AppSettings, TextAssistSettings, DEFAULT_CLEANUP_MODEL, RETIRED_CLEANUP_MODELS,
        TEXT_ASSIST_CHORD,
    },
};

/// Text Assist presets make settings files larger than before; 256 KB leaves ample room.
const MAX_SETTINGS_BYTES: u64 = 256 * 1024;

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
        Ok(mut settings) => {
            if migrate_settings(&mut settings) {
                // Persist deliberate migrations so the file states the model in use.
                // A failed write keeps the migrated value for this session only.
                let _ = save_settings_to(path, &settings);
            }
            Ok((settings, None))
        }
        Err(_) => {
            let backup =
                path.with_file_name(format!("settings-invalid-{}.json", uuid::Uuid::new_v4()));
            // Text Assist is the newest and largest block. When only it is unreadable, keep
            // every other setting and reset just Text Assist (the original is preserved).
            if contents.len() as u64 <= MAX_SETTINGS_BYTES {
                if let Some(mut settings) = without_text_assist(&contents) {
                    fs::copy(path, &backup).context("Could not preserve invalid settings file")?;
                    migrate_settings(&mut settings);
                    let _ = save_settings_to(path, &settings);
                    return Ok((
                        settings,
                        Some("Vellora could not read your saved Text Assist settings. They were reset to the defaults; all other settings were kept. The original file was preserved as settings-invalid-*.json in the app data folder.".to_string()),
                    ));
                }
            }
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

/// The settings without their `textAssist` block (defaults instead), when everything else
/// parses and validates. `None` when the rest is unreadable too or there was no such block.
fn without_text_assist(contents: &[u8]) -> Option<AppSettings> {
    let mut value: serde_json::Value = serde_json::from_slice(contents).ok()?;
    value.as_object_mut()?.remove("textAssist")?;
    let settings: AppSettings = serde_json::from_value(value).ok()?;
    validate_settings(&settings).ok()?;
    Some(settings)
}

/// Saved settings must stay loadable: the loader refuses files over [`MAX_SETTINGS_BYTES`].
fn ensure_loadable_size(contents: &str) -> anyhow::Result<()> {
    if contents.len() as u64 > MAX_SETTINGS_BYTES {
        anyhow::bail!(
            "Settings are too large to save ({} KB, limit {} KB). Shorten or remove some presets, custom instructions or endpoints.",
            contents.len().div_ceil(1024),
            MAX_SETTINGS_BYTES / 1024
        );
    }
    Ok(())
}

pub fn save_settings(settings: &AppSettings) -> anyhow::Result<()> {
    let path = settings_path()?;
    save_settings_to(&path, settings)
}

/// Applies deliberate upgrades to settings saved by older versions. Returns true when
/// anything changed. Only retired values are replaced; other preferences stay as saved.
pub fn migrate_settings(settings: &mut AppSettings) -> bool {
    if RETIRED_CLEANUP_MODELS.contains(&settings.cleanup_model.as_str()) {
        settings.cleanup_model = DEFAULT_CLEANUP_MODEL.to_string();
        return true;
    }
    false
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
    validate_text_assist(&settings.text_assist)?;
    Ok(())
}

/// Checks Text Assist values only. A failure on load preserves the user's file, so rules
/// never depend on file-system or network state (CLI paths are checked when the user sets
/// them). Messages never include the rejected value.
fn validate_text_assist(assist: &TextAssistSettings) -> anyhow::Result<()> {
    anyhow::ensure!(
        assist.chord == TEXT_ASSIST_CHORD,
        "Text Assist shortcut must be Shift+Win"
    );
    anyhow::ensure!(
        (150..=1500).contains(&assist.hold_threshold_ms),
        "Text Assist hold threshold must be between 150 and 1500 ms"
    );
    validate_model_ref(&assist.quick_model)?;

    anyhow::ensure!(
        (1..=MAX_PRESETS).contains(&assist.presets.len()),
        "Text Assist needs between 1 and 20 presets"
    );
    let mut preset_ids = HashSet::new();
    for preset in &assist.presets {
        anyhow::ensure!(
            is_identifier(&preset.id),
            "Preset IDs must be 1 to 40 lowercase letters, digits, '-' or '_'"
        );
        anyhow::ensure!(
            preset_ids.insert(preset.id.as_str()),
            "Preset IDs must be unique"
        );
        anyhow::ensure!(
            (1..=40).contains(&preset.name.chars().count()),
            "Preset names must be 1 to 40 characters"
        );
        anyhow::ensure!(
            preset.instruction.chars().count() <= 2_000,
            "Preset instructions must be at most 2000 characters"
        );
        if let Some(model) = &preset.model {
            validate_model_ref(model)?;
        }
        if let Some(color) = &preset.color {
            anyhow::ensure!(is_hex_color(color), "Preset colors must be #rrggbb");
        }
    }
    anyhow::ensure!(
        (1..=40).contains(&assist.last_preset_id.chars().count()),
        "Last preset ID must be 1 to 40 characters"
    );

    anyhow::ensure!(
        assist.favorite_models.len() <= 12,
        "Text Assist allows at most 12 favorite models"
    );
    for model in &assist.favorite_models {
        validate_model_ref(model)?;
    }

    anyhow::ensure!(
        assist.ensemble_profiles.len() <= 20,
        "Text Assist allows at most 20 ensemble profiles"
    );
    let mut profile_ids = HashSet::new();
    for profile in &assist.ensemble_profiles {
        anyhow::ensure!(
            is_identifier(&profile.id),
            "Ensemble profile IDs must be 1 to 40 lowercase letters, digits, '-' or '_'"
        );
        anyhow::ensure!(
            profile_ids.insert(profile.id.as_str()),
            "Ensemble profile IDs must be unique"
        );
        anyhow::ensure!(
            (1..=40).contains(&profile.name.trim().chars().count()),
            "Ensemble profile names must be 1 to 40 characters"
        );
        anyhow::ensure!(
            (1..=9).contains(&profile.members.len()),
            "Ensemble profiles need between 1 and 9 models"
        );
        for model in &profile.members {
            validate_model_ref(model)?;
        }
    }

    anyhow::ensure!(
        assist.model_colors.len() <= 64,
        "Text Assist allows at most 64 model colors"
    );
    for (key, color) in &assist.model_colors {
        anyhow::ensure!(
            key.chars().count() <= 200,
            "Model color keys must be at most 200 characters"
        );
        anyhow::ensure!(is_hex_color(color), "Model colors must be #rrggbb");
    }

    let limits = &assist.limits;
    anyhow::ensure!(
        (100..=50_000).contains(&limits.quick_max_chars),
        "Quick Polish limit must be between 100 and 50000 characters"
    );
    anyhow::ensure!(
        (100..=200_000).contains(&limits.panel_max_chars),
        "Rewrite panel limit must be between 100 and 200000 characters"
    );
    anyhow::ensure!(
        limits.ensemble_max_usd_per_run.is_finite()
            && (0.0..=5.0).contains(&limits.ensemble_max_usd_per_run),
        "Ensemble cost limit must be between 0 and 5 USD"
    );
    anyhow::ensure!(
        (50..=5_000).contains(&limits.paste_restore_delay_ms),
        "Clipboard restore delay must be between 50 and 5000 ms"
    );

    anyhow::ensure!(
        assist.custom_instructions.len() <= 5,
        "Text Assist keeps at most 5 custom instructions"
    );
    anyhow::ensure!(
        assist
            .custom_instructions
            .iter()
            .all(|instruction| instruction.chars().count() <= 500),
        "Custom instructions must be at most 500 characters"
    );

    anyhow::ensure!(
        assist.openai_compatible.len() <= 10,
        "Text Assist allows at most 10 OpenAI-compatible endpoints"
    );
    let mut endpoint_ids = HashSet::new();
    for endpoint in &assist.openai_compatible {
        anyhow::ensure!(
            is_identifier(&endpoint.id),
            "Endpoint IDs must be 1 to 40 lowercase letters, digits, '-' or '_'"
        );
        anyhow::ensure!(
            endpoint_ids.insert(endpoint.id.as_str()),
            "Endpoint IDs must be unique"
        );
        anyhow::ensure!(
            (1..=40).contains(&endpoint.name.trim().chars().count()),
            "Endpoint names must be 1 to 40 characters"
        );
        anyhow::ensure!(
            is_allowed_base_url(&endpoint.base_url),
            "Endpoint URLs must use HTTPS, or HTTP only for 127.0.0.1, localhost or [::1]"
        );
        anyhow::ensure!(
            endpoint.models.len() <= 50,
            "Endpoints allow at most 50 models"
        );
        anyhow::ensure!(
            endpoint.models.iter().all(|model| is_model_id(model)),
            "Endpoint model IDs must be 1 to 128 characters of letters, digits, '.', '_', ':', '/' or '-'"
        );
    }

    for bridge in [&assist.cli.codex, &assist.cli.claude] {
        if let Some(path) = &bridge.path {
            anyhow::ensure!(
                path.chars().count() <= 260 && Path::new(path).is_absolute(),
                "CLI paths must be absolute and at most 260 characters"
            );
        }
    }
    Ok(())
}

const MAX_PRESETS: usize = 20;

fn validate_model_ref(model: &ModelRef) -> anyhow::Result<()> {
    anyhow::ensure!(
        is_model_id(&model.model),
        "Model IDs must be 1 to 128 characters of letters, digits, '.', '_', ':', '/' or '-'"
    );
    if let Some(endpoint) = &model.endpoint {
        anyhow::ensure!(
            is_identifier(endpoint),
            "Endpoint IDs must be 1 to 40 lowercase letters, digits, '-' or '_'"
        );
    }
    Ok(())
}

/// `[A-Za-z0-9._:/-]{1,128}`; `/` allows OpenRouter-style IDs.
fn is_model_id(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'/' | b'-')
        })
}

/// `[a-z0-9_-]{1,40}`
fn is_identifier(value: &str) -> bool {
    (1..=40).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

/// `#rrggbb`
fn is_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// HTTPS anywhere; plain HTTP only for loopback hosts.
fn is_allowed_base_url(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    // Credentials, queries and fragments never belong in a base URL (keys live in the
    // Credential Manager); the engine rejects them too.
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    match url.scheme() {
        "https" => url.host_str().is_some_and(|host| !host.is_empty()),
        "http" => matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")),
        _ => false,
    }
}

fn save_settings_to(path: &Path, settings: &AppSettings) -> anyhow::Result<()> {
    validate_settings(settings)?;
    let contents =
        serde_json::to_string_pretty(settings).context("Could not serialize settings")?;
    ensure_loadable_size(&contents)?;
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
    fn retired_cleanup_model_migrates_and_keeps_other_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let saved = AppSettings {
            language: "de".to_string(),
            cleanup_enabled: true,
            save_history: false,
            cleanup_model: "gpt-5-nano-2025-08-07".to_string(),
            ..AppSettings::default()
        };
        save_settings_to(&path, &saved).unwrap();
        let (loaded, warning) = load_settings_from(&path).unwrap();
        assert!(warning.is_none());
        assert_eq!(loaded.cleanup_model, DEFAULT_CLEANUP_MODEL);
        assert_eq!(loaded.language, "de");
        assert!(loaded.cleanup_enabled);
        assert!(!loaded.save_history);
        // The migration is written back, so the next load is already current.
        let on_disk: AppSettings = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(on_disk.cleanup_model, DEFAULT_CLEANUP_MODEL);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn migration_leaves_current_and_custom_cleanup_models_unchanged() {
        for model in [DEFAULT_CLEANUP_MODEL, "gpt-5.6-luna", "my-custom-model"] {
            let mut settings = AppSettings {
                cleanup_model: model.to_string(),
                ..AppSettings::default()
            };
            assert!(!migrate_settings(&mut settings));
            assert_eq!(settings.cleanup_model, model);
        }
        let mut settings = AppSettings {
            cleanup_model: "gpt-5-nano".to_string(),
            ..AppSettings::default()
        };
        assert!(migrate_settings(&mut settings));
        assert_eq!(settings.cleanup_model, DEFAULT_CLEANUP_MODEL);
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

    use crate::{
        text_providers::{Effort, ProviderKind},
        types::{CompatEndpoint, EnsembleProfile, RewritePreset},
    };

    /// A settings file as written by the current release: no `textAssist` key.
    const RELEASE_SETTINGS_JSON: &str = r#"{
  "appearance": "classic",
  "provider": "openai",
  "sttModel": "gpt-4o-transcribe",
  "cleanupEnabled": true,
  "cleanupMode": "light",
  "cleanupModel": "gpt-6-luna",
  "autoPaste": false,
  "copyToClipboard": true,
  "saveHistory": true,
  "language": "de",
  "hotkey": "Ctrl+Win"
}"#;

    fn model(id: &str) -> ModelRef {
        ModelRef::new(ProviderKind::Openai, id, Some(Effort::Low))
    }

    fn with_assist(change: impl FnOnce(&mut TextAssistSettings)) -> AppSettings {
        let mut settings = AppSettings::default();
        change(&mut settings.text_assist);
        settings
    }

    #[test]
    fn default_text_assist_settings_validate() {
        validate_settings(&AppSettings::default()).unwrap();
    }

    #[test]
    fn release_settings_file_loads_unchanged_with_text_assist_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, RELEASE_SETTINGS_JSON).unwrap();
        let (loaded, warning) = load_settings_from(&path).unwrap();
        assert!(warning.is_none());
        assert_eq!(loaded.language, "de");
        assert!(!loaded.auto_paste);
        assert_eq!(loaded.text_assist, TextAssistSettings::default());
        // Nothing was migrated, so the file stays exactly as the release wrote it.
        assert_eq!(fs::read_to_string(&path).unwrap(), RELEASE_SETTINGS_JSON);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn partial_text_assist_file_loads_and_fills_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut json: serde_json::Value = serde_json::from_str(RELEASE_SETTINGS_JSON).unwrap();
        json["textAssist"] = serde_json::json!({
            "holdThresholdMs": 600,
            "quickModel": { "provider": "openai", "model": "gpt-5.6-luna" },
            "limits": { "pasteRestoreDelayMs": 800 }
        });
        fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        let (loaded, warning) = load_settings_from(&path).unwrap();
        assert!(warning.is_none());
        let defaults = TextAssistSettings::default();
        let assist = &loaded.text_assist;
        assert_eq!(assist.hold_threshold_ms, 600);
        assert_eq!(assist.quick_model.model, "gpt-5.6-luna");
        assert_eq!(assist.quick_model.effort, None);
        assert_eq!(assist.limits.paste_restore_delay_ms, 800);
        assert_eq!(
            assist.limits.quick_max_chars,
            defaults.limits.quick_max_chars
        );
        assert_eq!(assist.presets, defaults.presets);
        assert_eq!(assist.chord, defaults.chord);
    }

    #[test]
    fn text_assist_settings_round_trip_through_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = with_assist(|assist| {
            assist.enabled = false;
            assist.presets.push(RewritePreset {
                id: "shorter".to_string(),
                name: "Shorter".to_string(),
                instruction: "Make it shorter.".to_string(),
                model: Some(model("openrouter/some-vendor/model:free")),
                color: Some("#A1b2C3".to_string()),
                builtin: false,
            });
            assist.ensemble_profiles.push(EnsembleProfile {
                id: "deep".to_string(),
                name: "Deep academic".to_string(),
                members: vec![model("gpt-6-luna"), model("gpt-5.6-luna")],
            });
            assist.model_colors.insert(
                "anthropic/claude-opus-5-5".to_string(),
                "#d97757".to_string(),
            );
            assist.openai_compatible.push(CompatEndpoint {
                id: "ollama".to_string(),
                name: "Ollama".to_string(),
                base_url: "http://localhost:11434/v1".to_string(),
                models: vec!["llama3.3:70b".to_string()],
            });
            assist.cli.codex.path = Some(r"C:\Tools\codex.exe".to_string());
            assist.custom_instructions = vec!["Translate to English".to_string()];
        });
        save_settings_to(&path, &settings).unwrap();
        let (loaded, warning) = load_settings_from(&path).unwrap();
        assert!(warning.is_none());
        assert_eq!(loaded.text_assist, settings.text_assist);
    }

    #[test]
    fn each_text_assist_rule_rejects_a_bad_value() {
        let long = |n: usize| "a".repeat(n);
        let cases: Vec<(&str, AppSettings)> = vec![
            ("chord", with_assist(|a| a.chord = "Ctrl+Win".to_string())),
            ("hold low", with_assist(|a| a.hold_threshold_ms = 149)),
            ("hold high", with_assist(|a| a.hold_threshold_ms = 1501)),
            (
                "quick model empty",
                with_assist(|a| a.quick_model.model.clear()),
            ),
            (
                "quick model chars",
                with_assist(|a| a.quick_model.model = "gpt 6".to_string()),
            ),
            (
                "quick model long",
                with_assist(|a| a.quick_model.model = long(129)),
            ),
            (
                "endpoint id",
                with_assist(|a| a.quick_model.endpoint = Some("Ollama".to_string())),
            ),
            (
                "endpoint id long",
                with_assist(|a| a.quick_model.endpoint = Some(long(41))),
            ),
            ("no presets", with_assist(|a| a.presets.clear())),
            (
                "too many presets",
                with_assist(|a| {
                    a.presets = (0..21)
                        .map(|i| RewritePreset {
                            id: format!("p{i}"),
                            name: format!("P{i}"),
                            ..RewritePreset::default()
                        })
                        .collect()
                }),
            ),
            (
                "preset id chars",
                with_assist(|a| a.presets[1].id = "Clearer".to_string()),
            ),
            ("preset id empty", with_assist(|a| a.presets[1].id.clear())),
            (
                "preset id duplicate",
                with_assist(|a| a.presets[1].id = "polish".to_string()),
            ),
            (
                "preset name empty",
                with_assist(|a| a.presets[1].name.clear()),
            ),
            (
                "preset name long",
                with_assist(|a| a.presets[1].name = long(41)),
            ),
            (
                "preset instruction long",
                with_assist(|a| a.presets[1].instruction = long(2001)),
            ),
            (
                "preset model",
                with_assist(|a| a.presets[1].model = Some(model("bad model"))),
            ),
            (
                "preset color",
                with_assist(|a| a.presets[1].color = Some("red".to_string())),
            ),
            (
                "preset color short",
                with_assist(|a| a.presets[1].color = Some("#fff".to_string())),
            ),
            (
                "last preset empty",
                with_assist(|a| a.last_preset_id.clear()),
            ),
            (
                "last preset long",
                with_assist(|a| a.last_preset_id = long(41)),
            ),
            (
                "too many favorites",
                with_assist(|a| a.favorite_models = vec![model("m"); 13]),
            ),
            (
                "favorite model",
                with_assist(|a| a.favorite_models.push(model("m;drop"))),
            ),
            (
                "too many profiles",
                with_assist(|a| {
                    a.ensemble_profiles = vec![
                        EnsembleProfile {
                            id: "p".to_string(),
                            name: "P".to_string(),
                            members: vec![model("m")],
                        };
                        21
                    ]
                }),
            ),
            (
                "empty profile",
                with_assist(|a| {
                    a.ensemble_profiles.push(EnsembleProfile {
                        id: "p".to_string(),
                        name: "P".to_string(),
                        members: Vec::new(),
                    })
                }),
            ),
            (
                "profile too large",
                with_assist(|a| {
                    a.ensemble_profiles.push(EnsembleProfile {
                        id: "p".to_string(),
                        name: "P".to_string(),
                        members: vec![model("m"); 10],
                    })
                }),
            ),
            (
                "profile member",
                with_assist(|a| {
                    a.ensemble_profiles.push(EnsembleProfile {
                        id: "p".to_string(),
                        name: "P".to_string(),
                        members: vec![model("")],
                    })
                }),
            ),
            (
                "profile id",
                with_assist(|a| {
                    a.ensemble_profiles.push(EnsembleProfile {
                        id: "Bad Id".to_string(),
                        name: "P".to_string(),
                        members: vec![model("m")],
                    })
                }),
            ),
            (
                "duplicate profile id",
                with_assist(|a| {
                    let profile = EnsembleProfile {
                        id: "p".to_string(),
                        name: "P".to_string(),
                        members: vec![model("m")],
                    };
                    a.ensemble_profiles = vec![profile.clone(), profile];
                }),
            ),
            (
                "profile name",
                with_assist(|a| {
                    a.ensemble_profiles.push(EnsembleProfile {
                        id: "p".to_string(),
                        name: " ".to_string(),
                        members: vec![model("m")],
                    })
                }),
            ),
            (
                "model color",
                with_assist(|a| {
                    a.model_colors
                        .insert("openai/x".to_string(), "#12345g".to_string());
                }),
            ),
            (
                "model color key",
                with_assist(|a| {
                    a.model_colors.insert(long(201), "#123456".to_string());
                }),
            ),
            (
                "too many model colors",
                with_assist(|a| {
                    for i in 0..65 {
                        a.model_colors
                            .insert(format!("k{i}"), "#123456".to_string());
                    }
                }),
            ),
            (
                "quick limit low",
                with_assist(|a| a.limits.quick_max_chars = 99),
            ),
            (
                "quick limit high",
                with_assist(|a| a.limits.quick_max_chars = 50_001),
            ),
            (
                "panel limit low",
                with_assist(|a| a.limits.panel_max_chars = 99),
            ),
            (
                "panel limit high",
                with_assist(|a| a.limits.panel_max_chars = 200_001),
            ),
            (
                "ensemble usd negative",
                with_assist(|a| a.limits.ensemble_max_usd_per_run = -0.01),
            ),
            (
                "ensemble usd high",
                with_assist(|a| a.limits.ensemble_max_usd_per_run = 5.01),
            ),
            (
                "ensemble usd nan",
                with_assist(|a| a.limits.ensemble_max_usd_per_run = f64::NAN),
            ),
            (
                "ensemble usd infinite",
                with_assist(|a| a.limits.ensemble_max_usd_per_run = f64::INFINITY),
            ),
            (
                "restore low",
                with_assist(|a| a.limits.paste_restore_delay_ms = 49),
            ),
            (
                "restore high",
                with_assist(|a| a.limits.paste_restore_delay_ms = 5_001),
            ),
            (
                "too many custom instructions",
                with_assist(|a| a.custom_instructions = vec!["x".to_string(); 6]),
            ),
            (
                "custom instruction long",
                with_assist(|a| a.custom_instructions = vec![long(501)]),
            ),
            (
                "too many endpoints",
                with_assist(|a| {
                    a.openai_compatible = (0..11)
                        .map(|i| CompatEndpoint {
                            id: format!("e{i}"),
                            base_url: "https://example.com/v1".to_string(),
                            ..CompatEndpoint::default()
                        })
                        .collect()
                }),
            ),
            (
                "endpoint duplicate",
                with_assist(|a| {
                    let endpoint = CompatEndpoint {
                        id: "dup".to_string(),
                        base_url: "https://example.com/v1".to_string(),
                        ..CompatEndpoint::default()
                    };
                    a.openai_compatible = vec![endpoint.clone(), endpoint];
                }),
            ),
            (
                "endpoint bad id",
                with_assist(|a| {
                    a.openai_compatible.push(CompatEndpoint {
                        id: "My Server".to_string(),
                        base_url: "https://example.com/v1".to_string(),
                        ..CompatEndpoint::default()
                    })
                }),
            ),
            (
                "endpoint too many models",
                with_assist(|a| {
                    a.openai_compatible.push(CompatEndpoint {
                        id: "e".to_string(),
                        base_url: "https://example.com/v1".to_string(),
                        models: vec!["m".to_string(); 51],
                        ..CompatEndpoint::default()
                    })
                }),
            ),
            (
                "endpoint bad model",
                with_assist(|a| {
                    a.openai_compatible.push(CompatEndpoint {
                        id: "e".to_string(),
                        base_url: "https://example.com/v1".to_string(),
                        models: vec!["bad model".to_string()],
                        ..CompatEndpoint::default()
                    })
                }),
            ),
            (
                "cli path relative",
                with_assist(|a| a.cli.claude.path = Some(r"tools\claude.exe".to_string())),
            ),
            (
                "cli path long",
                with_assist(|a| a.cli.codex.path = Some(format!(r"C:\{}", long(260)))),
            ),
        ];
        for (name, settings) in cases {
            assert!(validate_settings(&settings).is_err(), "{name} was accepted");
        }
    }

    #[test]
    fn endpoint_urls_require_https_or_loopback_http() {
        let with_url = |url: &str| {
            with_assist(|a| {
                a.openai_compatible.push(CompatEndpoint {
                    id: "e".to_string(),
                    name: "Endpoint".to_string(),
                    base_url: url.to_string(),
                    ..CompatEndpoint::default()
                })
            })
        };
        for url in [
            "https://api.example.com/v1",
            "http://127.0.0.1:11434/v1",
            "http://localhost:1234/v1",
            "http://[::1]:8080/v1",
        ] {
            validate_settings(&with_url(url)).unwrap_or_else(|_| panic!("{url} rejected"));
        }
        for url in [
            "",
            "example.com",
            "http://example.com/v1",
            "http://192.168.1.5:11434/v1",
            "http://127.0.0.2/v1",
            "ftp://example.com",
            "file:///C:/x",
            "https://user:secret@example.com/v1",
            "https://example.com/v1?key=secret",
            "https://example.com/v1#frag",
        ] {
            assert!(validate_settings(&with_url(url)).is_err(), "{url} accepted");
        }
    }

    #[test]
    fn unknown_last_preset_and_missing_cli_path_are_not_validation_errors() {
        let settings = with_assist(|a| {
            a.last_preset_id = "deleted-preset".to_string();
            a.cli.claude.enabled = true;
            a.cli.claude.path = Some(r"C:\does\not\exist\claude.exe".to_string());
        });
        validate_settings(&settings).unwrap();
    }

    #[test]
    fn invalid_text_assist_block_on_load_preserves_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut json: serde_json::Value = serde_json::from_str(RELEASE_SETTINGS_JSON).unwrap();
        json["textAssist"] = serde_json::json!({ "holdThresholdMs": 5 });
        let contents = serde_json::to_vec(&json).unwrap();
        fs::write(&path, &contents).unwrap();
        let (loaded, warning) = load_settings_from(&path).unwrap();
        assert!(warning.is_some());
        assert_eq!(loaded.text_assist, TextAssistSettings::default());
        let backup = fs::read_dir(dir.path())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(fs::read(backup).unwrap(), contents);
    }

    #[test]
    fn settings_files_up_to_256_kb_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut contents = RELEASE_SETTINGS_JSON.to_string();
        contents.push_str(&" ".repeat(200 * 1024));
        fs::write(&path, &contents).unwrap();
        let (loaded, warning) = load_settings_from(&path).unwrap();
        assert!(warning.is_none());
        assert_eq!(loaded.language, "de");
    }

    #[test]
    fn settings_too_large_to_load_are_not_saved() {
        assert!(ensure_loadable_size(&"x".repeat(MAX_SETTINGS_BYTES as usize)).is_ok());
        let err = ensure_loadable_size(&"x".repeat(MAX_SETTINGS_BYTES as usize + 1)).unwrap_err();
        assert!(err.to_string().contains("too large"));
    }

    #[test]
    fn invalid_text_assist_resets_only_text_assist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = AppSettings {
            language: "de".to_string(),
            ..AppSettings::default()
        };
        let mut value = serde_json::to_value(&settings).unwrap();
        value["textAssist"]["holdThresholdMs"] = serde_json::json!("not a number");
        let contents = serde_json::to_vec_pretty(&value).unwrap();
        fs::write(&path, &contents).unwrap();

        let (loaded, warning) = load_settings_from(&path).unwrap();
        assert!(warning.unwrap().contains("Text Assist settings"));
        assert_eq!(loaded.language, "de");
        assert_eq!(loaded.text_assist, TextAssistSettings::default());
        // The original is preserved next to the repaired file.
        let backups: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("settings-invalid-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read(&backups[0]).unwrap(), contents);
        let (reloaded, warning) = load_settings_from(&path).unwrap();
        assert!(warning.is_none());
        assert_eq!(reloaded.language, "de");
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
