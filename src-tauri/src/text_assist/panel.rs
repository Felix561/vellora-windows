//! Rewrite panel (spec 2.2, M4). Holding Shift+Win captures the selection and opens a focusable
//! panel next to it. Presets stream results into the panel; Apply hands focus back to the editor,
//! re-copies to verify the selection is unchanged and replaces it.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::Utc;
use serde::Serialize;
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize};

use super::{
    geometry::{self, Anchor, PointPx, RectPx},
    guard::{self, PreparedSelection},
    job::{self, JobPlatform, Phase},
    platform::{self, TargetSnapshot, WindowsPlatform},
    prompts,
    runtime::{self, Runtime},
};
use crate::{
    clipboard, clipboard_snapshot,
    history::{
        self, TextAssistRun, KIND_ENSEMBLE, KIND_REWRITE, OUTCOME_APPLIED, OUTCOME_FAILED,
        OUTCOME_KEPT,
    },
    text_providers::{
        catalog, Billing, CancelToken, Effort, FinishReason, ModelRef, ProviderKind, TextError,
        TextRequest,
    },
    types::{AppSettings, Appearance},
};

pub const PANEL_WINDOW: &str = "assist-panel";
pub const PANEL_OPEN_EVENT: &str = "text-assist-panel-open";
pub const PANEL_EVENT: &str = "text-assist-panel";
/// Panel request budget (spec 3.4).
pub const PANEL_TIMEOUT: Duration = Duration::from_secs(90);
/// Panel size in logical (96 DPI) pixels; matches `tauri.conf.json`.
const PANEL_SIZE: (f64, f64) = (560.0, 420.0);
const STREAM_INTERVAL: Duration = Duration::from_millis(60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelPreset {
    pub id: String,
    pub name: String,
    pub instruction: String,
    pub builtin: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelModel {
    pub key: String,
    pub label: String,
    pub effort: Option<String>,
    pub color: String,
    pub billing: &'static str,
}

/// Sent once when the panel opens (event and `get_panel_init`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelInit {
    pub session_id: String,
    pub original: String,
    pub target_app: Option<String>,
    pub appearance: &'static str,
    pub presets: Vec<PanelPreset>,
    pub active_preset_id: String,
    pub models: Vec<PanelModel>,
    pub active_model_index: usize,
    pub custom_instructions: Vec<String>,
    pub max_chars: usize,
    /// Ensemble profiles (M7). When none are configured, one implicit "Favorites" profile.
    pub ensemble_profiles: Vec<PanelEnsembleProfile>,
    pub ensemble_max_usd: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelEnsembleProfile {
    pub id: String,
    pub name: String,
    pub members: Vec<PanelModel>,
}

/// Shown before an ensemble run: members, providers and the estimated API cost (spec 2.3).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnsemblePreview {
    pub profile_id: String,
    pub members: Vec<PanelModel>,
    pub providers: Vec<String>,
    pub estimated_usd: f64,
    /// True when a paid member has no catalog price (the estimate excludes it).
    pub unknown_cost: bool,
    pub limit_usd: f64,
    pub blocked: bool,
}

/// One request's state; mirrors `PanelResult` in `src/lib/assistPanel.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelResult {
    pub session_id: String,
    pub request_id: String,
    pub preset_id: String,
    pub model_index: usize,
    pub phase: &'static str,
    pub text: String,
    pub label: String,
    pub effort: Option<String>,
    pub billing: &'static str,
    pub color: String,
    pub duration_ms: Option<u64>,
    pub cost_usd: Option<f64>,
    pub warnings: Vec<&'static str>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ensemble_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_index: Option<usize>,
}

#[derive(Debug, Clone)]
struct Stored {
    result: PanelResult,
    run_id: Option<String>,
}

struct Session {
    init: PanelInit,
    target: TargetSnapshot,
    original: String,
    prepared: PreparedSelection,
    models: Vec<ModelRef>,
    /// Ensemble profiles: id -> members.
    profiles: Vec<(String, Vec<ModelRef>)>,
    /// `preset|model|instruction` -> request id of the finished result.
    cache: HashMap<String, String>,
    results: HashMap<String, Stored>,
    running: Option<CancelToken>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn require_panel(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == PANEL_WINDOW {
        Ok(())
    } else {
        Err("This action is only available in the Rewrite panel.".to_string())
    }
}

/// Where the panel goes: below the selection, else above it, clamped to the work area.
pub(crate) fn panel_rect(anchor: &Anchor, work: RectPx, scale: f64) -> RectPx {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let width = ((PANEL_SIZE.0 * scale).round() as i32).min(work.width.max(1));
    let height = ((PANEL_SIZE.1 * scale).round() as i32).min(work.height.max(1));
    let gap = (10.0 * scale).round() as i32;
    let below = anchor.rect.bottom() + gap;
    let above = anchor.rect.y - gap - height;
    let y = if below + height <= work.bottom() {
        below
    } else if above >= work.y {
        above
    } else {
        below.clamp(work.y, work.bottom() - height)
    };
    let x = anchor.rect.x.clamp(work.x, work.right() - width);
    RectPx::new(x, y, width, height)
}

fn panel_models(settings: &AppSettings) -> Vec<ModelRef> {
    let assist = &settings.text_assist;
    let mut models: Vec<ModelRef> = Vec::new();
    if assist.dev_offline_transform {
        models.push(ModelRef::new(
            ProviderKind::Offline,
            crate::text_providers::offline::OFFLINE_MODEL_ID,
            None,
        ));
    }
    for model in std::iter::once(&assist.quick_model).chain(assist.favorite_models.iter()) {
        if !models.contains(model) {
            models.push(model.clone());
        }
    }
    models
}

fn presets(settings: &AppSettings) -> Vec<PanelPreset> {
    settings
        .text_assist
        .presets
        .iter()
        .map(|preset| PanelPreset {
            id: preset.id.clone(),
            name: preset.name.clone(),
            instruction: preset.instruction.clone(),
            builtin: preset.builtin,
        })
        .collect()
}

fn panel_model(settings: &AppSettings, model: &ModelRef) -> PanelModel {
    PanelModel {
        key: format!(
            "{}/{}",
            model.color_key(),
            model.effort.map_or("default", |effort| effort.as_str())
        ),
        label: runtime::model_label(model),
        effort: model.effort.map(|effort| effort.as_str().to_string()),
        color: runtime::model_color(settings, model),
        billing: model.provider.billing().as_str(),
    }
}

/// Maximum members running at once, and of those at most two subscription CLIs (spec M7).
const ENSEMBLE_PARALLEL: usize = 4;
const ENSEMBLE_PARALLEL_CLI: usize = 2;
const ENSEMBLE_MAX_MEMBERS: usize = 9;

/// Configured ensemble profiles, or an implicit "Favorites" profile with up to four distinct
/// models from the quick model and favorites.
fn ensemble_profiles(settings: &AppSettings) -> Vec<(String, String, Vec<ModelRef>)> {
    let configured: Vec<_> = settings
        .text_assist
        .ensemble_profiles
        .iter()
        .filter(|profile| !profile.members.is_empty())
        .map(|profile| {
            let members = profile
                .members
                .iter()
                .take(ENSEMBLE_MAX_MEMBERS)
                .cloned()
                .collect();
            (profile.id.clone(), profile.name.clone(), members)
        })
        .collect();
    if !configured.is_empty() {
        return configured;
    }
    let mut members: Vec<ModelRef> = Vec::new();
    let assist = &settings.text_assist;
    for model in std::iter::once(&assist.quick_model).chain(assist.favorite_models.iter()) {
        if !members.contains(model) && members.len() < ENSEMBLE_PARALLEL {
            members.push(model.clone());
        }
    }
    vec![("favorites".to_string(), "Favorites".to_string(), members)]
}

fn is_cli(provider: ProviderKind) -> bool {
    matches!(provider, ProviderKind::CodexCli | ProviderKind::ClaudeCli)
}

/// Conservative per-member estimate in USD: input = (instructions + text) / 3 tokens, output =
/// 1.3 x the text plus a reasoning allowance per effort. Plan and local members cost nothing
/// extra; `None` for a paid model without a catalog price.
pub(crate) fn estimate_member_usd(
    model: &ModelRef,
    instruction_chars: usize,
    input_chars: usize,
) -> Option<f64> {
    if model.provider.billing() != Billing::Api {
        return Some(0.0);
    }
    let entry = catalog::find(model.provider, &model.model)?;
    let (input_price, output_price) = catalog::current_prices(entry);
    let input_tokens = ((instruction_chars + input_chars) as f64 / 3.0).ceil();
    let reasoning = match model.effort {
        None | Some(Effort::None) | Some(Effort::Minimal) => 0.0,
        Some(Effort::Low) => 1_000.0,
        Some(Effort::Medium) => 3_000.0,
        Some(Effort::High) => 8_000.0,
        Some(Effort::Xhigh) | Some(Effort::Max) => 16_000.0,
    };
    let output_tokens = (input_chars as f64 / 3.0 * 1.3).ceil() + reasoning;
    Some(input_tokens * input_price / 1e6 + output_tokens * output_price / 1e6)
}

/// Sums member estimates and decides whether the per-run limit blocks the run.
pub(crate) fn ensemble_cost(
    members: &[ModelRef],
    instruction_chars: usize,
    input_chars: usize,
    limit_usd: f64,
) -> (f64, bool, bool) {
    let mut total = 0.0;
    let mut unknown = false;
    for member in members {
        match estimate_member_usd(member, instruction_chars, input_chars) {
            Some(cost) => total += cost,
            None => unknown = true,
        }
    }
    (total, unknown, total > limit_usd)
}

/// Opens a panel session for the current foreground selection. Runs on the chord worker thread.
pub(crate) fn open(rt: &Arc<Runtime>) {
    close_session(rt, false);
    let settings = (rt.settings)();
    let platform = WindowsPlatform;
    let target = match platform.snapshot() {
        Ok(target) => target,
        Err(failure) => {
            rt.show_status(None, Phase::Failed, "rewrite", "#8a8f98", &failure.message);
            return;
        }
    };
    let limit = settings.text_assist.limits.panel_max_chars;
    let original = match platform.capture(&target, limit) {
        Ok(text) => text,
        Err(failure) => {
            rt.show_status(
                Some(&target.anchor),
                Phase::Failed,
                "rewrite",
                "#8a8f98",
                &failure.message,
            );
            return;
        }
    };
    let models = panel_models(&settings);
    let presets = presets(&settings);
    let profiles = ensemble_profiles(&settings);
    let active_preset_id = presets
        .iter()
        .find(|preset| preset.id == settings.text_assist.last_preset_id)
        .or_else(|| presets.first())
        .map(|preset| preset.id.clone())
        .unwrap_or_else(|| prompts::PRESET_POLISH.to_string());
    let init = PanelInit {
        session_id: uuid::Uuid::new_v4().to_string(),
        original: original.clone(),
        target_app: target.exe_name.clone(),
        appearance: match settings.appearance {
            Appearance::Notebook => "notebook",
            Appearance::Classic => "classic",
        },
        presets,
        active_preset_id,
        models: models
            .iter()
            .map(|model| panel_model(&settings, model))
            .collect(),
        active_model_index: 0,
        custom_instructions: settings.text_assist.custom_instructions.clone(),
        max_chars: limit,
        ensemble_profiles: profiles
            .iter()
            .map(|(id, name, members)| PanelEnsembleProfile {
                id: id.clone(),
                name: name.clone(),
                members: members
                    .iter()
                    .map(|model| panel_model(&settings, model))
                    .collect(),
            })
            .collect(),
        ensemble_max_usd: settings.text_assist.limits.ensemble_max_usd_per_run,
    };
    let anchor = target.anchor;
    *SESSION.lock().unwrap() = Some(Session {
        prepared: guard::prepare(&original),
        init: init.clone(),
        target,
        original,
        models,
        profiles: profiles
            .into_iter()
            .map(|(id, _, members)| (id, members))
            .collect(),
        cache: HashMap::new(),
        results: HashMap::new(),
        running: None,
    });
    rt.halo.hide_now();
    show_panel_window(rt, &anchor);
    let _ = rt.halo.app.emit_to(PANEL_WINDOW, PANEL_OPEN_EVENT, &init);
}

fn show_panel_window(rt: &Runtime, anchor: &Anchor) {
    let Some(window) = rt.halo.app.get_webview_window(PANEL_WINDOW) else {
        return;
    };
    let center = PointPx {
        x: anchor.rect.x + anchor.rect.width / 2,
        y: anchor.rect.y + anchor.rect.height / 2,
    };
    let (work, scale) = geometry::monitor_for(center);
    let rect = panel_rect(anchor, work, scale);
    let _ = window.set_position(PhysicalPosition::new(rect.x, rect.y));
    let _ = window.set_size(PhysicalSize::new(rect.width as u32, rect.height as u32));
    // M1: taking the foreground needs the Alt + mask-key unlock first.
    let _ = clipboard_snapshot::send_foreground_unlock();
    let _ = window.show();
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();
}

fn hide_panel_window(rt: &Runtime) {
    if let Some(window) = rt.halo.app.get_webview_window(PANEL_WINDOW) {
        let _ = window.hide();
    }
}

/// Ends the session (Esc, close button, or a new panel). Optionally returns focus.
fn close_session(rt: &Runtime, return_focus: bool) {
    let session = SESSION.lock().unwrap().take();
    hide_panel_window(rt);
    if let Some(session) = session {
        if let Some(cancel) = session.running {
            cancel.cancel();
        }
        if return_focus {
            let target = session.target;
            std::thread::spawn(move || {
                platform::return_focus(&target);
            });
        }
    }
}

fn emit(rt: &Runtime, result: &PanelResult) {
    let _ = rt.halo.app.emit_to(PANEL_WINDOW, PANEL_EVENT, result);
}

struct PanelJob {
    base: PanelResult,
    model: ModelRef,
    preset_id: String,
    instruction: String,
    prepared: PreparedSelection,
    original: String,
    target_app: Option<String>,
    cache_key: String,
    kind: &'static str,
    ensemble_run_id: Option<String>,
}

fn record(rt: &Runtime, job: &PanelJob, run: TextAssistRun) -> Option<String> {
    let id = run.id.clone();
    if !(rt.settings)().text_assist.save_history {
        return None;
    }
    if let Err(err) = history::insert_text_assist_run(&run) {
        eprintln!("Could not save Text Assist history: {err:#}");
        return None;
    }
    let _ = rt
        .halo
        .app
        .emit_to("main", runtime::RUN_RECORDED_EVENT, &id);
    let _ = job;
    Some(id)
}

fn history_row(
    job: &PanelJob,
    result: Option<String>,
    outcome: &str,
    reason: Option<&str>,
    started: Instant,
    usage: Option<crate::text_providers::TokenUsage>,
    first_token_ms: Option<u64>,
) -> TextAssistRun {
    TextAssistRun {
        id: uuid::Uuid::new_v4().to_string(),
        created_at: Utc::now().to_rfc3339(),
        kind: job.kind.to_string(),
        preset_id: Some(job.preset_id.clone()),
        ensemble_run_id: job.ensemble_run_id.clone(),
        provider: job.model.provider.as_str().to_string(),
        model: job.model.model.clone(),
        effort: job.model.effort.map(|effort| effort.as_str().to_string()),
        billing: job.model.provider.billing().as_str().to_string(),
        original_text: job.original.clone(),
        result_text: result,
        outcome: outcome.to_string(),
        reason_code: reason.map(str::to_string),
        target_app: job.target_app.clone(),
        duration_ms: Some(started.elapsed().as_millis() as u64),
        first_token_ms,
        input_tokens: usage.map(|usage| usage.input_tokens),
        output_tokens: usage.map(|usage| usage.output_tokens),
        estimated_cost_usd: usage.and_then(|usage| catalog::estimate_cost_usd(&job.model, &usage)),
    }
}

fn store(job: &PanelJob, result: &PanelResult, run_id: Option<String>) {
    let mut guard = SESSION.lock().unwrap();
    let Some(session) = guard
        .as_mut()
        .filter(|session| session.init.session_id == result.session_id)
    else {
        return;
    };
    session.results.insert(
        result.request_id.clone(),
        Stored {
            result: result.clone(),
            run_id,
        },
    );
    if result.phase == "done" {
        session
            .cache
            .insert(job.cache_key.clone(), result.request_id.clone());
    }
}

async fn run_request(rt: Arc<Runtime>, job: PanelJob, cancel: CancelToken) {
    let started = Instant::now();
    emit(&rt, &job.base);
    let engine = match rt.engine_for(&job.model) {
        Ok(engine) => engine,
        Err(error) => {
            let result = PanelResult {
                phase: "failed",
                error: Some(job::engine_error_label(&error)),
                ..job.base.clone()
            };
            store(&job, &result, None);
            emit(&rt, &result);
            return;
        }
    };
    let text = Mutex::new(String::new());
    let last = Mutex::new(None::<Instant>);
    let on_delta = |piece: &str| {
        let mut text = text.lock().unwrap();
        text.push_str(piece);
        let now = Instant::now();
        let mut last = last.lock().unwrap();
        if last.is_some_and(|at| now.duration_since(at) < STREAM_INTERVAL) {
            return;
        }
        *last = Some(now);
        emit(
            &rt,
            &PanelResult {
                phase: "streaming",
                text: text.clone(),
                ..job.base.clone()
            },
        );
    };
    let request = TextRequest {
        model: job.model.clone(),
        instructions: prompts::instructions_for(&job.preset_id, &job.instruction),
        input: job.prepared.core.clone(),
        max_output_tokens: prompts::max_output_tokens(&job.prepared.core),
        timeout: PANEL_TIMEOUT,
    };
    let outcome = engine.stream(request, cancel, &on_delta).await;
    let duration_ms = Some(started.elapsed().as_millis() as u64);
    match outcome {
        Ok(outcome) => {
            let completed = outcome.finish == FinishReason::Completed;
            let restored = guard::restore(&job.prepared, &outcome.text);
            let report = guard::check(&job.prepared.core, outcome.text.trim(), completed);
            let cost = outcome
                .usage
                .and_then(|usage| catalog::estimate_cost_usd(&job.model, &usage));
            let usable = completed && !restored.trim().is_empty();
            let result = PanelResult {
                phase: if usable { "done" } else { "failed" },
                text: restored.clone(),
                duration_ms,
                cost_usd: cost,
                warnings: report
                    .issues
                    .iter()
                    .map(|issue| issue.reason_code())
                    .collect(),
                error: (!usable).then(|| "Output incomplete".to_string()),
                ..job.base.clone()
            };
            let row = history_row(
                &job,
                Some(restored),
                if usable { OUTCOME_KEPT } else { OUTCOME_FAILED },
                report.reason_code(),
                started,
                outcome.usage,
                outcome.first_token_ms,
            );
            let run_id = record(&rt, &job, row);
            store(&job, &result, run_id);
            emit(&rt, &result);
        }
        Err(TextError::Cancelled) => {
            // Switching presets cancels the previous request; that is not worth a history row.
            emit(
                &rt,
                &PanelResult {
                    phase: "cancelled",
                    duration_ms,
                    ..job.base.clone()
                },
            );
        }
        Err(error) => {
            let result = PanelResult {
                phase: "failed",
                duration_ms,
                error: Some(job::engine_error_label(&error)),
                ..job.base.clone()
            };
            let row = history_row(
                &job,
                None,
                OUTCOME_FAILED,
                Some(error.reason_code()),
                started,
                None,
                None,
            );
            let run_id = record(&rt, &job, row);
            store(&job, &result, run_id);
            emit(&rt, &result);
        }
    }
}

fn remember_choice(rt: &Runtime, preset_id: &str, custom: Option<&str>) {
    let preset_id = preset_id.to_string();
    let custom = custom.map(str::to_string);
    (rt.update_settings)(&move |settings: &mut AppSettings| {
        settings.text_assist.last_preset_id = preset_id.clone();
        if let Some(custom) = &custom {
            let list = &mut settings.text_assist.custom_instructions;
            list.retain(|item| item != custom);
            list.insert(0, custom.chars().take(500).collect());
            list.truncate(5);
        }
    });
}

#[tauri::command]
pub fn get_panel_init(window: tauri::WebviewWindow) -> Result<Option<PanelInit>, String> {
    require_panel(&window)?;
    Ok(SESSION
        .lock()
        .unwrap()
        .as_ref()
        .map(|session| session.init.clone()))
}

#[tauri::command]
pub fn panel_run(
    window: tauri::WebviewWindow,
    preset_id: String,
    model_index: usize,
    custom_instruction: Option<String>,
    fresh: bool,
) -> Result<String, String> {
    require_panel(&window)?;
    let rt = runtime::current().ok_or("Text Assist is not running")?;
    let request_id = uuid::Uuid::new_v4().to_string();
    let mut guard = SESSION.lock().unwrap();
    let session = guard.as_mut().ok_or("The panel is closed")?;
    let preset = session
        .init
        .presets
        .iter()
        .find(|preset| preset.id == preset_id)
        .cloned()
        .ok_or("Unknown preset")?;
    let model = session
        .models
        .get(model_index)
        .cloned()
        .ok_or("Unknown model")?;
    let custom = custom_instruction
        .map(|text| text.trim().chars().take(500).collect::<String>())
        .filter(|text| !text.is_empty());
    let is_custom = preset.id == prompts::PRESET_CUSTOM;
    let instruction = if is_custom {
        custom.clone().ok_or("Type an instruction first")?
    } else {
        preset.instruction.clone()
    };
    if let Some(cancel) = session.running.take() {
        cancel.cancel();
    }
    let info = &session.init.models[model_index];
    let base = PanelResult {
        session_id: session.init.session_id.clone(),
        request_id: request_id.clone(),
        preset_id: preset.id.clone(),
        model_index,
        phase: "thinking",
        text: String::new(),
        label: info.label.clone(),
        effort: info.effort.clone(),
        billing: info.billing,
        color: info.color.clone(),
        duration_ms: None,
        cost_usd: None,
        warnings: Vec::new(),
        error: None,
        ensemble_run_id: None,
        member_index: None,
    };
    let cache_key = format!("{}|{}|{}", preset.id, model_index, instruction);
    if !fresh {
        let cached = session
            .cache
            .get(&cache_key)
            .and_then(|id| session.results.get(id))
            .cloned();
        if let Some(stored) = cached {
            let result = PanelResult {
                request_id: request_id.clone(),
                ..stored.result
            };
            session.results.insert(
                request_id.clone(),
                Stored {
                    result: result.clone(),
                    run_id: stored.run_id,
                },
            );
            drop(guard);
            emit(&rt, &result);
            remember_choice(&rt, &preset.id, None);
            return Ok(request_id);
        }
    }
    let cancel = CancelToken::new();
    session.running = Some(cancel.clone());
    let job = PanelJob {
        base,
        model,
        preset_id: preset.id.clone(),
        instruction,
        prepared: session.prepared.clone(),
        original: session.original.clone(),
        target_app: session.target.exe_name.clone(),
        cache_key,
        kind: KIND_REWRITE,
        ensemble_run_id: None,
    };
    drop(guard);
    remember_choice(
        &rt,
        &preset.id,
        if is_custom { custom.as_deref() } else { None },
    );
    tauri::async_runtime::spawn(run_request(rt, job, cancel));
    Ok(request_id)
}

/// Resolves a preset's instruction (Custom uses the typed instruction).
fn preset_instruction(
    session: &Session,
    preset_id: &str,
    custom: Option<String>,
) -> Result<(PanelPreset, String, Option<String>), String> {
    let preset = session
        .init
        .presets
        .iter()
        .find(|preset| preset.id == preset_id)
        .cloned()
        .ok_or("Unknown preset")?;
    let custom = custom
        .map(|text| text.trim().chars().take(500).collect::<String>())
        .filter(|text| !text.is_empty());
    let instruction = if preset.id == prompts::PRESET_CUSTOM {
        custom.clone().ok_or("Type an instruction first")?
    } else {
        preset.instruction.clone()
    };
    Ok((preset, instruction, custom))
}

fn preview_for(
    session: &Session,
    settings: &AppSettings,
    profile_id: &str,
    instruction: &str,
) -> Result<(Vec<ModelRef>, EnsemblePreview), String> {
    let members = session
        .profiles
        .iter()
        .find(|(id, _)| id == profile_id)
        .map(|(_, members)| members.clone())
        .ok_or("Unknown ensemble profile")?;
    if members.is_empty() {
        return Err("This ensemble profile has no members".to_string());
    }
    let instruction_chars = prompts::instructions_for(prompts::PRESET_CUSTOM, instruction)
        .chars()
        .count();
    let input_chars = session.prepared.core.chars().count();
    let limit = settings.text_assist.limits.ensemble_max_usd_per_run;
    let (estimated, unknown, blocked) =
        ensemble_cost(&members, instruction_chars, input_chars, limit);
    let mut providers: Vec<String> = members
        .iter()
        .map(|model| model.provider.as_str().to_string())
        .collect();
    providers.dedup();
    Ok((
        members.clone(),
        EnsemblePreview {
            profile_id: profile_id.to_string(),
            members: members
                .iter()
                .map(|model| panel_model(settings, model))
                .collect(),
            providers,
            estimated_usd: estimated,
            unknown_cost: unknown,
            limit_usd: limit,
            blocked,
        },
    ))
}

#[tauri::command]
pub fn panel_ensemble_preview(
    window: tauri::WebviewWindow,
    profile_id: String,
    preset_id: String,
    custom_instruction: Option<String>,
) -> Result<EnsemblePreview, String> {
    require_panel(&window)?;
    let rt = runtime::current().ok_or("Text Assist is not running")?;
    let settings = (rt.settings)();
    let guard = SESSION.lock().unwrap();
    let session = guard.as_ref().ok_or("The panel is closed")?;
    let (_, instruction, _) = preset_instruction(session, &preset_id, custom_instruction)?;
    preview_for(session, &settings, &profile_id, &instruction).map(|(_, preview)| preview)
}

/// Runs every member of an ensemble profile on the same preset (spec 2.3). Returns the
/// ensemble run id; each member streams `PanelResult`s with `ensembleRunId`/`memberIndex`.
#[tauri::command]
pub fn panel_run_ensemble(
    window: tauri::WebviewWindow,
    profile_id: String,
    preset_id: String,
    custom_instruction: Option<String>,
) -> Result<String, String> {
    require_panel(&window)?;
    let rt = runtime::current().ok_or("Text Assist is not running")?;
    let settings = (rt.settings)();
    let mut guard = SESSION.lock().unwrap();
    let session = guard.as_mut().ok_or("The panel is closed")?;
    let (preset, instruction, custom) =
        preset_instruction(session, &preset_id, custom_instruction)?;
    let (members, preview) = preview_for(session, &settings, &profile_id, &instruction)?;
    if preview.blocked {
        return Err(format!(
            "Estimated ${:.4} exceeds the per-run limit of ${:.2}",
            preview.estimated_usd, preview.limit_usd
        ));
    }
    if let Some(cancel) = session.running.take() {
        cancel.cancel();
    }
    let cancel = CancelToken::new();
    session.running = Some(cancel.clone());
    let ensemble_run_id = uuid::Uuid::new_v4().to_string();
    let jobs: Vec<PanelJob> = members
        .iter()
        .enumerate()
        .map(|(index, model)| {
            let info = &preview.members[index];
            let model_index = session
                .models
                .iter()
                .position(|candidate| candidate == model)
                .unwrap_or(usize::MAX);
            PanelJob {
                base: PanelResult {
                    session_id: session.init.session_id.clone(),
                    request_id: uuid::Uuid::new_v4().to_string(),
                    preset_id: preset.id.clone(),
                    model_index,
                    phase: "thinking",
                    text: String::new(),
                    label: info.label.clone(),
                    effort: info.effort.clone(),
                    billing: info.billing,
                    color: info.color.clone(),
                    duration_ms: None,
                    cost_usd: None,
                    warnings: Vec::new(),
                    error: None,
                    ensemble_run_id: Some(ensemble_run_id.clone()),
                    member_index: Some(index),
                },
                model: model.clone(),
                preset_id: preset.id.clone(),
                instruction: instruction.clone(),
                prepared: session.prepared.clone(),
                original: session.original.clone(),
                target_app: session.target.exe_name.clone(),
                cache_key: format!("ensemble|{ensemble_run_id}|{index}"),
                kind: KIND_ENSEMBLE,
                ensemble_run_id: Some(ensemble_run_id.clone()),
            }
        })
        .collect();
    drop(guard);
    let is_custom = preset.id == prompts::PRESET_CUSTOM;
    remember_choice(
        &rt,
        &preset.id,
        if is_custom { custom.as_deref() } else { None },
    );
    tauri::async_runtime::spawn(async move {
        let all = Arc::new(tokio::sync::Semaphore::new(ENSEMBLE_PARALLEL));
        let cli = Arc::new(tokio::sync::Semaphore::new(ENSEMBLE_PARALLEL_CLI));
        let mut handles = Vec::new();
        for job in jobs {
            // Show every card at once; queued members wait for a slot.
            emit(&rt, &job.base);
            let (rt, cancel, all, cli) = (rt.clone(), cancel.clone(), all.clone(), cli.clone());
            handles.push(tauri::async_runtime::spawn(async move {
                let _slot = all.acquire_owned().await;
                let _cli_slot = if is_cli(job.model.provider) {
                    Some(cli.acquire_owned().await)
                } else {
                    None
                };
                if cancel.is_cancelled() {
                    emit(
                        &rt,
                        &PanelResult {
                            phase: "cancelled",
                            ..job.base.clone()
                        },
                    );
                    return;
                }
                run_request(rt, job, cancel).await;
            }));
        }
        for handle in handles {
            let _ = handle.await;
        }
    });
    Ok(ensemble_run_id)
}

#[tauri::command]
pub async fn panel_apply(window: tauri::WebviewWindow, request_id: String) -> Result<(), String> {
    require_panel(&window)?;
    let rt = runtime::current().ok_or("Text Assist is not running")?;
    let (session, stored) = {
        let mut guard = SESSION.lock().unwrap();
        let session = guard.as_ref().ok_or("The panel is closed")?;
        let stored = session
            .results
            .get(&request_id)
            .filter(|stored| stored.result.phase == "done" && !stored.result.text.is_empty())
            .cloned()
            .ok_or("This result is not ready")?;
        let session = guard.take().expect("checked above");
        (session, stored)
    };
    if let Some(cancel) = &session.running {
        cancel.cancel();
    }
    hide_panel_window(&rt);
    let delay = Duration::from_millis((rt.settings)().text_assist.limits.paste_restore_delay_ms);
    let target = session.target.clone();
    let original = session.original.clone();
    let text = stored.result.text.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        WindowsPlatform.apply_from_panel(&target, &original, &text, delay)
    })
    .await
    .map_err(|_| "Text Assist stopped unexpectedly".to_string())?;
    let color = stored.result.color.clone();
    match outcome {
        Ok(()) => {
            if let Some(run_id) = &stored.run_id {
                let _ = history::update_text_assist_run_outcome(run_id, OUTCOME_APPLIED, None);
            }
            rt.show_status(
                Some(&session.target.anchor),
                Phase::Done,
                "rewrite",
                &color,
                "Applied",
            );
        }
        Err(failure) => {
            if let Some(run_id) = &stored.run_id {
                let _ = history::update_text_assist_run_outcome(
                    run_id,
                    OUTCOME_KEPT,
                    Some(failure.code),
                );
            }
            rt.show_status(
                Some(&session.target.anchor),
                Phase::Kept,
                "rewrite",
                &color,
                "Saved in Vellora",
            );
        }
    }
    let _ = rt
        .halo
        .app
        .emit_to("main", runtime::RUN_RECORDED_EVENT, &request_id);
    Ok(())
}

#[tauri::command]
pub fn panel_copy(window: tauri::WebviewWindow, request_id: String) -> Result<(), String> {
    require_panel(&window)?;
    let text = SESSION
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|session| session.results.get(&request_id))
        .map(|stored| stored.result.text.clone())
        .filter(|text| !text.is_empty())
        .ok_or("Nothing to copy yet")?;
    clipboard::copy_text(&text).map_err(|err| format!("{err:#}"))
}

#[tauri::command]
pub fn panel_close(window: tauri::WebviewWindow) -> Result<(), String> {
    require_panel(&window)?;
    if let Some(rt) = runtime::current() {
        close_session(&rt, true);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_assist::geometry::AnchorKind;

    fn anchor(x: i32, y: i32, width: i32, height: i32) -> Anchor {
        Anchor {
            kind: AnchorKind::Selection,
            rect: RectPx::new(x, y, width, height),
        }
    }

    #[test]
    fn panel_goes_below_the_selection_when_it_fits() {
        let work = RectPx::new(0, 0, 1920, 1040);
        let rect = panel_rect(&anchor(300, 200, 400, 40), work, 1.0);
        assert_eq!(rect, RectPx::new(300, 250, 560, 420));
        // 150% scale grows the panel and the gap.
        let rect = panel_rect(&anchor(300, 200, 400, 40), work, 1.5);
        assert_eq!(rect, RectPx::new(300, 255, 840, 630));
    }

    #[test]
    fn panel_goes_above_near_the_bottom_and_is_clamped_horizontally() {
        let work = RectPx::new(0, 0, 1920, 1040);
        let rect = panel_rect(&anchor(1700, 900, 100, 30), work, 1.0);
        assert_eq!(rect, RectPx::new(1360, 470, 560, 420));
    }

    #[test]
    fn panel_is_clamped_when_neither_side_fits() {
        let work = RectPx::new(-1280, 0, 1280, 600);
        let rect = panel_rect(&anchor(-1200, 250, 50, 20), work, 1.0);
        assert_eq!(rect.x, -1200);
        assert!(rect.y >= work.y && rect.bottom() <= work.bottom());
        assert_eq!(rect.height, 420);
    }

    #[test]
    fn implicit_ensemble_uses_up_to_four_distinct_models() {
        let mut settings = AppSettings::default();
        let profiles = ensemble_profiles(&settings);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].0, "favorites");
        assert_eq!(profiles[0].2.len(), 2);
        for model in ["a", "b", "c", "d", "e"] {
            settings.text_assist.favorite_models.push(ModelRef::new(
                ProviderKind::Openai,
                model,
                None,
            ));
        }
        assert_eq!(ensemble_profiles(&settings)[0].2.len(), 4);
        settings
            .text_assist
            .ensemble_profiles
            .push(crate::types::EnsembleProfile {
                id: "deep".to_string(),
                name: "Deep".to_string(),
                members: vec![ModelRef::new(ProviderKind::ClaudeCli, "opus", None)],
            });
        let profiles = ensemble_profiles(&settings);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].0, "deep");
    }

    #[test]
    fn ensemble_cost_sums_paid_members_and_blocks_over_the_limit() {
        let luna = ModelRef::new(ProviderKind::Openai, "gpt-6-luna", Some(Effort::None));
        let plan = ModelRef::new(ProviderKind::ClaudeCli, "opus", None);
        let unknown = ModelRef::new(ProviderKind::Openai, "not-in-catalog", None);
        // 3,000 chars of text: input (300 + 3,000) / 3 = 1,100 tokens; output 1,300 tokens.
        let one = estimate_member_usd(&luna, 300, 3_000).unwrap();
        assert!((one - (1_100.0 * 0.10 + 1_300.0 * 0.50) / 1e6).abs() < 1e-12);
        assert_eq!(estimate_member_usd(&plan, 300, 3_000), Some(0.0));
        assert_eq!(estimate_member_usd(&unknown, 300, 3_000), None);
        let (total, unknown_cost, blocked) =
            ensemble_cost(&[luna.clone(), plan, unknown], 300, 3_000, 0.05);
        assert!((total - one).abs() < 1e-12);
        assert!(unknown_cost);
        assert!(!blocked);
        let opus = ModelRef::new(
            ProviderKind::Anthropic,
            "claude-opus-5-5",
            Some(Effort::Max),
        );
        let (_, _, blocked) = ensemble_cost(&[opus.clone(), opus], 300, 30_000, 0.05);
        assert!(blocked);
    }

    #[test]
    fn models_start_with_quick_model_and_drop_duplicates() {
        let mut settings = AppSettings::default();
        let quick = settings.text_assist.quick_model.clone();
        settings.text_assist.favorite_models = vec![
            quick.clone(),
            settings.text_assist.favorite_models[1].clone(),
        ];
        let models = panel_models(&settings);
        assert_eq!(models[0], quick);
        assert_eq!(models.len(), 2);
        settings.text_assist.dev_offline_transform = true;
        assert_eq!(panel_models(&settings)[0].provider, ProviderKind::Offline);
    }
}
