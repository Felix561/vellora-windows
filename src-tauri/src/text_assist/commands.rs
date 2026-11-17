//! Tauri commands for the Text Assist settings, history and dashboard UI. Main window only.

use serde::Serialize;

use super::runtime;
use crate::{
    clipboard, credentials,
    credentials::ProviderKey,
    history,
    history::{TextAssistCostTotals, TextAssistRun},
    text_providers::{catalog, Effort, ProviderKind},
};

/// A catalog model as shown in the model pickers.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextModelInfo {
    pub provider: ProviderKind,
    pub id: String,
    pub label: String,
    pub efforts: Vec<Effort>,
    pub default_effort: Option<Effort>,
    pub input_usd_per_million: f64,
    pub output_usd_per_million: f64,
    pub color: String,
    pub quick_candidate: bool,
    pub shutdown: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextAssistOverview {
    pub totals: TextAssistCostTotals,
    pub models: Vec<TextModelInfo>,
    pub last_kept: Option<TextAssistRun>,
}

fn models() -> Vec<TextModelInfo> {
    catalog::models()
        .iter()
        .filter(|model| model.provider != ProviderKind::Offline)
        .map(|model| {
            // Prices in effect today (Gemini Flash prices change on 2027-01-01).
            let (input, output) = catalog::current_prices(model);
            (model, input, output)
        })
        .map(|(model, input, output)| TextModelInfo {
            provider: model.provider,
            id: model.id.to_string(),
            label: model.label.to_string(),
            efforts: model.efforts.to_vec(),
            default_effort: model.default_effort,
            input_usd_per_million: input,
            output_usd_per_million: output,
            color: model.color.to_string(),
            quick_candidate: model.quick_candidate,
            shutdown: model.shutdown.map(str::to_string),
        })
        .collect()
}

fn to_string(err: anyhow::Error) -> String {
    format!("{err:#}")
}

#[tauri::command]
pub fn list_text_assist_runs(
    window: tauri::WebviewWindow,
    limit: Option<usize>,
) -> Result<Vec<TextAssistRun>, String> {
    crate::require_main_window(&window)?;
    history::list_text_assist_runs(limit.unwrap_or(100).clamp(1, 500)).map_err(to_string)
}

#[tauri::command]
pub fn delete_text_assist_run(window: tauri::WebviewWindow, id: String) -> Result<bool, String> {
    crate::require_main_window(&window)?;
    history::delete_text_assist_run(&id).map_err(to_string)
}

#[tauri::command]
pub fn clear_text_assist_runs(window: tauri::WebviewWindow) -> Result<usize, String> {
    crate::require_main_window(&window)?;
    history::clear_text_assist_runs().map_err(to_string)
}

/// Copies a run's result (or original, when there is no result) to the clipboard.
#[tauri::command]
pub fn copy_text_assist_text(window: tauri::WebviewWindow, text: String) -> Result<(), String> {
    crate::require_main_window(&window)?;
    clipboard::copy_text(&text).map_err(to_string)
}

#[tauri::command]
pub fn get_text_assist_overview(
    window: tauri::WebviewWindow,
) -> Result<TextAssistOverview, String> {
    crate::require_main_window(&window)?;
    let today = chrono::Local::now().date_naive();
    Ok(TextAssistOverview {
        totals: history::text_assist_cost_totals(today).unwrap_or_default(),
        models: models(),
        last_kept: runtime::last_kept_result(),
    })
}

/// Which provider keys are saved (never the keys themselves).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextProviderKeys {
    pub openai: bool,
    pub anthropic: bool,
    pub gemini: bool,
    /// Endpoint id -> key saved, for the OpenAI-compatible endpoints in settings.
    pub compat: std::collections::BTreeMap<String, bool>,
}

fn provider_key(provider: &str, endpoint_id: Option<&str>) -> Result<ProviderKey, String> {
    match provider {
        "anthropic" => Ok(ProviderKey::Anthropic),
        "gemini" => Ok(ProviderKey::Gemini),
        "openai_compatible" => {
            let id = endpoint_id.ok_or("Choose an endpoint first")?;
            ProviderKey::compat(id).map_err(to_string)
        }
        _ => Err("Use the OpenAI key setting for OpenAI".to_string()),
    }
}

#[tauri::command]
pub fn get_text_provider_keys(
    window: tauri::WebviewWindow,
    endpoint_ids: Vec<String>,
) -> Result<TextProviderKeys, String> {
    crate::require_main_window(&window)?;
    let mut keys = TextProviderKeys {
        openai: credentials::get_openai_key().map_err(to_string)?.is_some(),
        anthropic: credentials::has_provider_key(&ProviderKey::Anthropic).map_err(to_string)?,
        gemini: credentials::has_provider_key(&ProviderKey::Gemini).map_err(to_string)?,
        ..TextProviderKeys::default()
    };
    for id in endpoint_ids.into_iter().take(10) {
        let key = ProviderKey::compat(&id).map_err(to_string)?;
        let saved = credentials::has_provider_key(&key).map_err(to_string)?;
        keys.compat.insert(id, saved);
    }
    Ok(keys)
}

#[tauri::command]
pub fn save_text_provider_key(
    window: tauri::WebviewWindow,
    provider: String,
    endpoint_id: Option<String>,
    key: String,
) -> Result<(), String> {
    crate::require_main_window(&window)?;
    let provider = provider_key(&provider, endpoint_id.as_deref())?;
    credentials::save_provider_key(&provider, key.trim()).map_err(to_string)
}

#[tauri::command]
pub fn delete_text_provider_key(
    window: tauri::WebviewWindow,
    provider: String,
    endpoint_id: Option<String>,
) -> Result<(), String> {
    crate::require_main_window(&window)?;
    let provider = provider_key(&provider, endpoint_id.as_deref())?;
    credentials::delete_provider_key(&provider).map_err(to_string)
}

/// Installed/version/signed-in status of the subscription CLIs (no model request is made).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextCliStatus {
    pub codex: crate::text_providers::cli_common::CliStatus,
    pub claude: crate::text_providers::cli_common::CliStatus,
}

#[tauri::command]
pub async fn get_text_cli_status(window: tauri::WebviewWindow) -> Result<TextCliStatus, String> {
    use crate::text_providers::cli_common::{cli_status, CliKind};
    crate::require_main_window(&window)?;
    let settings = super::runtime::current()
        .map(|runtime| (runtime.settings)())
        .unwrap_or_default();
    let cli = settings.text_assist.cli;
    let (codex, claude) = tokio::join!(
        cli_status(CliKind::Codex, cli.codex.path.as_deref()),
        cli_status(CliKind::Claude, cli.claude.path.as_deref())
    );
    Ok(TextCliStatus { codex, claude })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_key_names_map_and_refuse_openai() {
        assert!(matches!(
            provider_key("anthropic", None),
            Ok(ProviderKey::Anthropic)
        ));
        assert!(matches!(
            provider_key("gemini", None),
            Ok(ProviderKey::Gemini)
        ));
        assert!(provider_key("openai_compatible", None).is_err());
        assert!(provider_key("openai_compatible", Some("Bad Id")).is_err());
        assert!(provider_key("openai_compatible", Some("ollama")).is_ok());
        assert!(provider_key("openai", None).is_err());
    }

    #[test]
    fn model_list_hides_the_offline_engine_and_keeps_prices() {
        let list = models();
        assert!(list
            .iter()
            .all(|model| model.provider != ProviderKind::Offline));
        let luna = list
            .iter()
            .find(|model| model.id == "gpt-6-luna")
            .expect("gpt-6-luna in catalog");
        assert!(luna.quick_candidate);
        assert!(luna.efforts.contains(&Effort::None));
        assert!(luna.input_usd_per_million > 0.0);
    }
}
