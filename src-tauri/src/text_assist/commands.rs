//! Tauri commands for the Text Assist settings, history and dashboard UI. Main window only.

use serde::Serialize;

use super::runtime;
use crate::{
    clipboard, history,
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
        .map(|model| TextModelInfo {
            provider: model.provider,
            id: model.id.to_string(),
            label: model.label.to_string(),
            efforts: model.efforts.to_vec(),
            default_effort: model.default_effort,
            input_usd_per_million: model.input_usd_per_million,
            output_usd_per_million: model.output_usd_per_million,
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

#[cfg(test)]
mod tests {
    use super::*;

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
