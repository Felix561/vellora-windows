use std::fs;

use anyhow::Context;

use crate::{credentials, settings};

pub fn run_startup_migrations() -> Vec<String> {
    let mut warnings = Vec::new();
    if let Err(err) = migrate_settings() {
        warnings.push(format!("Settings migration warning: {err:#}"));
    }
    if let Err(err) = migrate_history() {
        warnings.push(format!("History migration warning: {err:#}"));
    }
    if let Err(err) = credentials::migrate_openai_key() {
        warnings.push(format!("Credential migration warning: {err:#}"));
    }
    warnings
}

fn migrate_settings() -> anyhow::Result<()> {
    let old_path = settings::legacy_settings_path()?;
    let new_path = settings::settings_path()?;
    if new_path.exists() || !old_path.exists() {
        return Ok(());
    }
    if let Some(parent) = new_path.parent() {
        fs::create_dir_all(parent).context("Could not create Vellora settings directory")?;
    }
    fs::copy(&old_path, &new_path).with_context(|| {
        format!(
            "Could not copy settings from {} to {}",
            old_path.display(),
            new_path.display()
        )
    })?;
    Ok(())
}

fn migrate_history() -> anyhow::Result<()> {
    let old_path = settings::legacy_app_data_dir()?.join(crate::brand::OLD_DATABASE_NAME);
    let new_path = settings::app_data_dir()?.join(crate::brand::DATABASE_NAME);
    if new_path.exists() || !old_path.exists() {
        return Ok(());
    }
    if let Some(parent) = new_path.parent() {
        fs::create_dir_all(parent).context("Could not create Vellora history directory")?;
    }
    fs::copy(&old_path, &new_path).with_context(|| {
        format!(
            "Could not copy history from {} to {}",
            old_path.display(),
            new_path.display()
        )
    })?;
    Ok(())
}
