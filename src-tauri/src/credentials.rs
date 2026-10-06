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
}
