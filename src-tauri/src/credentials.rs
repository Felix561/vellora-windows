use std::{fs, io::Write, path::Path};

use anyhow::Context;
use keyring::Entry;

use crate::{brand, settings};

fn entry() -> anyhow::Result<Entry> {
    Entry::new(brand::CREDENTIAL_SERVICE, brand::CREDENTIAL_USERNAME)
        .context("Could not open Windows Credential Manager entry")
}

fn old_entry() -> anyhow::Result<Entry> {
    Entry::new(brand::OLD_CREDENTIAL_SERVICE, brand::CREDENTIAL_USERNAME)
        .context("Could not open legacy Windows Credential Manager entry")
}

pub fn save_openai_key(key: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!key.trim().is_empty(), "API key cannot be empty");
    mark_migration_complete()?;
    entry()?
        .set_password(key)
        .context("Could not save OpenAI API key")
}

pub fn get_openai_key() -> anyhow::Result<Option<String>> {
    match entry()?.get_password() {
        Ok(key) if !key.trim().is_empty() => Ok(Some(key)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err).context("Could not read OpenAI API key"),
    }
}

pub fn delete_openai_key() -> anyhow::Result<()> {
    // An explicit removal must not be undone by importing the legacy key on restart.
    mark_migration_complete()?;
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err).context("Could not delete OpenAI API key"),
    }
}

/// A Text Assist provider key stored next to the OpenAI key, in the same Credential Manager
/// service. The OpenAI key keeps its existing entry and is not a `ProviderKey`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProviderKey {
    /// Anthropic API key (username `anthropic`).
    Anthropic,
    /// Google AI Studio key (username `gemini`).
    Gemini,
    /// Optional key for an OpenAI-compatible endpoint (username `compat:<id>`).
    /// The id must match `[a-z0-9_-]{1,40}`; use [`ProviderKey::compat`] to build one.
    Compat(String),
}

impl ProviderKey {
    /// Key for the compatible endpoint `id`, validated.
    pub fn compat(id: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(valid_compat_id(id), "Invalid endpoint id");
        Ok(ProviderKey::Compat(id.to_string()))
    }

    /// Credential Manager username. Fails for an invalid compatible endpoint id.
    pub fn username(&self) -> anyhow::Result<String> {
        match self {
            ProviderKey::Anthropic => Ok("anthropic".to_string()),
            ProviderKey::Gemini => Ok("gemini".to_string()),
            ProviderKey::Compat(id) => {
                anyhow::ensure!(valid_compat_id(id), "Invalid endpoint id");
                Ok(format!("compat:{id}"))
            }
        }
    }

    fn label(&self) -> &'static str {
        match self {
            ProviderKey::Anthropic => "Anthropic API key",
            ProviderKey::Gemini => "Gemini API key",
            ProviderKey::Compat(_) => "endpoint API key",
        }
    }
}

fn valid_compat_id(id: &str) -> bool {
    (1..=40).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

fn provider_entry(provider: &ProviderKey) -> anyhow::Result<Entry> {
    Entry::new(brand::CREDENTIAL_SERVICE, &provider.username()?)
        .context("Could not open Windows Credential Manager entry")
}

// Called by the Models tab key commands (M5 UI).
#[allow(dead_code)]
pub fn save_provider_key(provider: &ProviderKey, key: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!key.trim().is_empty(), "API key cannot be empty");
    provider_entry(provider)?
        .set_password(key)
        .with_context(|| format!("Could not save {}", provider.label()))
}

pub fn get_provider_key(provider: &ProviderKey) -> anyhow::Result<Option<String>> {
    match provider_entry(provider)?.get_password() {
        Ok(key) if !key.trim().is_empty() => Ok(Some(key)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err).with_context(|| format!("Could not read {}", provider.label())),
    }
}

// Called by the Models tab key commands (M5 UI).
#[allow(dead_code)]
pub fn delete_provider_key(provider: &ProviderKey) -> anyhow::Result<()> {
    match provider_entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err).with_context(|| format!("Could not delete {}", provider.label())),
    }
}

// Called by the Models tab key commands (M5 UI).
#[allow(dead_code)]
pub fn has_provider_key(provider: &ProviderKey) -> anyhow::Result<bool> {
    Ok(get_provider_key(provider)?.is_some())
}

fn migration_marker_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(settings::app_data_dir()?.join("credential-migration-complete"))
}

fn mark_migration_complete() -> anyhow::Result<()> {
    mark_at(&migration_marker_path()?)
}

fn mark_at(path: &Path) -> anyhow::Result<()> {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            file.write_all(b"Legacy credential import completed or explicitly disabled.\n")?;
            file.sync_all()
                .context("Could not save credential migration status")
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(err).context("Could not save credential migration status"),
    }
}

pub fn migrate_openai_key() -> anyhow::Result<()> {
    if migration_marker_path()?
        .try_exists()
        .context("Could not read credential migration status")?
    {
        return Ok(());
    }
    if get_openai_key()?.is_some() {
        return mark_migration_complete();
    }
    let old_key = match old_entry()?.get_password() {
        Ok(key) if !key.trim().is_empty() => Some(key),
        Ok(_) | Err(keyring::Error::NoEntry) => None,
        Err(err) => return Err(err).context("Could not read legacy OpenAI API key"),
    };
    if let Some(old_key) = old_key {
        entry()?
            .set_password(&old_key)
            .context("Could not migrate legacy API key")?;
    }
    mark_migration_complete()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_marker_is_idempotent_and_contains_no_key() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("credential-migration-complete");
        mark_at(&marker).unwrap();
        let original = fs::read(&marker).unwrap();
        mark_at(&marker).unwrap();
        assert_eq!(fs::read(&marker).unwrap(), original);
        assert!(!String::from_utf8(original).unwrap().contains("sk-"));
    }

    #[test]
    fn provider_key_usernames() {
        assert_eq!(ProviderKey::Anthropic.username().unwrap(), "anthropic");
        assert_eq!(ProviderKey::Gemini.username().unwrap(), "gemini");
        assert_eq!(
            ProviderKey::compat("ollama").unwrap().username().unwrap(),
            "compat:ollama"
        );
        assert_eq!(
            ProviderKey::compat("my_groq-2")
                .unwrap()
                .username()
                .unwrap(),
            "compat:my_groq-2"
        );
        let longest = "a".repeat(40);
        assert!(ProviderKey::compat(&longest).is_ok());
        // None of them collides with the existing OpenAI entry.
        for key in [ProviderKey::Anthropic, ProviderKey::Gemini] {
            assert_ne!(key.username().unwrap(), brand::CREDENTIAL_USERNAME);
        }
    }

    #[test]
    fn compat_ids_are_validated() {
        for bad in [
            "",
            "Ollama",
            "a b",
            "a:b",
            "../x",
            "x/y",
            "ü",
            "a.b",
            &"a".repeat(41),
        ] {
            assert!(ProviderKey::compat(bad).is_err(), "{bad}");
            // A directly built variant is checked again before any Credential Manager access.
            assert!(
                ProviderKey::Compat(bad.to_string()).username().is_err(),
                "{bad}"
            );
        }
    }
}
