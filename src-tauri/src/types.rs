use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    text_assist::prompts::{BUILTIN_PRESETS, PRESET_POLISH},
    text_providers::{Effort, ModelRef, ProviderKind},
};

/// Default dictation cleanup model. OpenAI names `gpt-6-luna` as the replacement for
/// `gpt-5-nano`, which shuts down on December 11, 2026 (checked November 17, 2026).
pub const DEFAULT_CLEANUP_MODEL: &str = "gpt-6-luna";

/// Cleanup model IDs that OpenAI retired or scheduled for shutdown. Saved settings that
/// still use one of them are migrated to [`DEFAULT_CLEANUP_MODEL`] on load.
pub const RETIRED_CLEANUP_MODELS: &[&str] = &["gpt-5-nano-2025-08-07", "gpt-5-nano"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub appearance: Appearance,
    pub provider: Provider,
    pub stt_model: SttModel,
    pub cleanup_enabled: bool,
    pub cleanup_mode: FixedCleanupMode,
    pub cleanup_model: String,
    pub auto_paste: bool,
    pub copy_to_clipboard: bool,
    pub save_history: bool,
    pub language: String,
    pub hotkey: String,
    /// Text Assist block (spec section 7.1). Absent in files written before Text Assist.
    #[serde(default)]
    pub text_assist: TextAssistSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            appearance: Appearance::Notebook,
            provider: Provider::Openai,
            stt_model: SttModel::Whisper1,
            cleanup_enabled: false,
            cleanup_mode: FixedCleanupMode::Light,
            cleanup_model: DEFAULT_CLEANUP_MODEL.to_string(),
            auto_paste: true,
            copy_to_clipboard: true,
            save_history: true,
            language: "auto".to_string(),
            hotkey: "Ctrl+Win".to_string(),
            text_assist: TextAssistSettings::default(),
        }
    }
}

/// The only Text Assist chord supported so far. Stored for future configurability.
pub const TEXT_ASSIST_CHORD: &str = "Shift+Win";
/// Default Quick Polish model.
/// Quick Polish default, chosen by the M3 benchmark (November 17, 2026): 100% exact, fastest.
pub const TEXT_ASSIST_QUICK_MODEL: &str = "gpt-5.6-luna";
/// Second default favorite model (Tab cycling in the Rewrite panel).
pub const TEXT_ASSIST_FAVORITE_MODEL: &str = "gpt-6-luna";

/// Text Assist settings, nested as `textAssist` in `settings.json` (spec section 7.1).
/// Every field falls back to its default, so partial blocks load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextAssistSettings {
    pub enabled: bool,
    pub chord: String,
    pub hold_threshold_ms: u64,
    pub quick_model: ModelRef,
    pub presets: Vec<RewritePreset>,
    pub last_preset_id: String,
    pub favorite_models: Vec<ModelRef>,
    pub ensemble_profiles: Vec<EnsembleProfile>,
    /// Per-model color overrides keyed by [`ModelRef::color_key`].
    pub model_colors: BTreeMap<String, String>,
    pub save_history: bool,
    pub limits: TextAssistLimits,
    pub cli: CliBridges,
    pub openai_compatible: Vec<CompatEndpoint>,
    /// Last custom instructions, newest first (at most five).
    pub custom_instructions: Vec<String>,
    /// Developer-only offline uppercase engine instead of a provider.
    pub dev_offline_transform: bool,
}

fn default_quick_model() -> ModelRef {
    ModelRef::new(
        ProviderKind::Openai,
        TEXT_ASSIST_QUICK_MODEL,
        Some(Effort::None),
    )
}

impl Default for TextAssistSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            chord: TEXT_ASSIST_CHORD.to_string(),
            hold_threshold_ms: 350,
            quick_model: default_quick_model(),
            presets: default_presets(),
            last_preset_id: PRESET_POLISH.to_string(),
            favorite_models: vec![
                default_quick_model(),
                ModelRef::new(
                    ProviderKind::Openai,
                    TEXT_ASSIST_FAVORITE_MODEL,
                    Some(Effort::Low),
                ),
            ],
            ensemble_profiles: Vec::new(),
            model_colors: BTreeMap::new(),
            save_history: true,
            limits: TextAssistLimits::default(),
            cli: CliBridges::default(),
            openai_compatible: Vec::new(),
            custom_instructions: Vec::new(),
            dev_offline_transform: false,
        }
    }
}

/// The built-in presets (spec section 6.4) as editable settings entries.
pub fn default_presets() -> Vec<RewritePreset> {
    BUILTIN_PRESETS
        .iter()
        .map(|preset| RewritePreset {
            id: preset.id.to_string(),
            name: preset.name.to_string(),
            instruction: preset.instruction.to_string(),
            model: None,
            color: None,
            builtin: true,
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RewritePreset {
    pub id: String,
    pub name: String,
    pub instruction: String,
    pub model: Option<ModelRef>,
    pub color: Option<String>,
    pub builtin: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EnsembleProfile {
    pub id: String,
    pub name: String,
    pub members: Vec<ModelRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextAssistLimits {
    pub quick_max_chars: usize,
    pub panel_max_chars: usize,
    pub ensemble_max_usd_per_run: f64,
    pub paste_restore_delay_ms: u64,
}

impl Default for TextAssistLimits {
    fn default() -> Self {
        Self {
            quick_max_chars: 8_000,
            panel_max_chars: 30_000,
            ensemble_max_usd_per_run: 0.05,
            paste_restore_delay_ms: 400,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CliBridges {
    pub codex: CliBridge,
    pub claude: CliBridge,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CliBridge {
    pub enabled: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CompatEndpoint {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub models: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Appearance {
    #[default]
    Notebook,
    Classic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Openai,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SttModel {
    #[serde(rename = "whisper-1")]
    Whisper1,
    #[serde(rename = "gpt-4o-mini-transcribe")]
    Gpt4oMiniTranscribe,
    #[serde(rename = "gpt-4o-transcribe")]
    Gpt4oTranscribe,
    #[serde(rename = "gpt-4o-transcribe-diarize")]
    Gpt4oTranscribeDiarize,
}

impl SttModel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Whisper1 => "whisper-1",
            Self::Gpt4oMiniTranscribe => "gpt-4o-mini-transcribe",
            Self::Gpt4oTranscribe => "gpt-4o-transcribe",
            Self::Gpt4oTranscribeDiarize => "gpt-4o-transcribe-diarize",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FixedCleanupMode {
    Light,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CleanupMode {
    Off,
    Light,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DictationState {
    Idle,
    Recording,
    Transcribing,
    Cleaning,
    Pasting,
    Done,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RecordingMode {
    Hold,
    Locked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptHistoryItem {
    pub id: String,
    pub created_at: String,
    pub provider: Provider,
    pub stt_model: SttModel,
    pub cleanup_model: Option<String>,
    pub cleanup_mode: CleanupMode,
    pub raw_text: String,
    pub final_text: String,
    pub duration_ms: Option<u64>,
    pub word_count: usize,
    pub char_count: usize,
    pub pasted: bool,
    pub error: Option<String>,
    pub pipeline_started_at: Option<String>,
    pub recording_duration_ms: Option<u64>,
    pub transcription_duration_ms: Option<u64>,
    pub cleanup_duration_ms: Option<u64>,
    pub paste_duration_ms: Option<u64>,
    pub total_pipeline_duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub state: DictationState,
    pub message: Option<String>,
    pub last_transcript: Option<TranscriptHistoryItem>,
    pub recording_mode: Option<RecordingMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioLevelEvent {
    pub level: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySnapshot {
    pub state: DictationState,
    pub message: Option<String>,
    pub level: Option<f32>,
    pub model: Option<String>,
    pub cleanup_enabled: Option<bool>,
    pub last_transcript_preview: Option<String>,
    pub recording_mode: Option<RecordingMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStats {
    pub total_words: usize,
    pub total_transcripts: usize,
    pub transcripts_today: usize,
    pub words_today: usize,
    pub estimated_minutes_saved: usize,
    pub average_words_per_transcript: usize,
    pub most_active_day: Option<String>,
    pub week: Vec<DailyStat>,
    pub model_usage: Vec<ModelUsageStat>,
    pub cost: CostSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyStat {
    pub date: String,
    pub day_label: String,
    pub transcript_count: usize,
    pub word_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageStat {
    pub model: String,
    pub transcript_count: usize,
    pub average_duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostSummary {
    pub pricing_version: String,
    pub estimated_total_usd: f64,
    pub estimated_today_usd: f64,
    pub estimated_week_usd: f64,
    pub estimated_transcription_usd: f64,
    pub estimated_cleanup_usd: f64,
    pub billable_audio_seconds: u64,
    pub measured_audio_seconds: u64,
    pub estimated_audio_seconds: u64,
    pub estimated_from_words_count: usize,
    pub unknown_model_count: usize,
    /// Cleanup runs whose model has no known price; their cleanup cost is not included.
    #[serde(default)]
    pub unknown_cleanup_model_count: usize,
    pub week: Vec<DailyCostStat>,
    pub model_breakdown: Vec<ModelCostStat>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyCostStat {
    pub date: String,
    pub day_label: String,
    pub estimated_cost_usd: f64,
    pub transcription_count: usize,
    pub audio_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCostStat {
    pub model: String,
    pub estimated_cost_usd: f64,
    pub transcription_count: usize,
    pub audio_seconds: u64,
}

// Usage overview (Dashboard): dictation and Text Assist side by side. Built by
// `usage_stats.rs`. Never carries selected, original or result text.

/// Assumptions behind the "time saved" estimates, shown in the UI as a hint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageAssumptions {
    /// Dictated words per minute of typing saved (same basis as `DashboardStats`).
    pub dictation_words_per_minute: u32,
    /// Seconds saved per corrected word in an applied Text Assist run.
    pub seconds_per_corrected_word: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationUsage {
    pub transcripts: usize,
    pub words: usize,
    pub minutes_saved: f64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextAssistUsage {
    /// Every recorded attempt, including failed ones that never captured text.
    pub runs: usize,
    pub applied: usize,
    pub failed: usize,
    /// Words in the captured selections (each ensemble selection counted once).
    pub words_processed: usize,
    /// Changed words in applied runs (word diff of original vs result).
    pub words_corrected: usize,
    pub minutes_saved: f64,
    /// Estimated API cost; plan and local runs add nothing.
    pub estimated_cost_usd: f64,
    /// Runs billed to a ChatGPT or Claude plan.
    pub plan_runs: usize,
    /// Distinct ensemble runs (one per `ensemble_run_id`).
    pub ensemble_runs: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextAssistOutcomeCounts {
    pub applied: usize,
    pub kept: usize,
    pub no_change: usize,
    pub cancelled: usize,
    pub failed: usize,
    pub other: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingSummary {
    /// Runs with a measured value.
    pub count: usize,
    pub average_ms: Option<u64>,
    pub median_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextAssistModelUsage {
    pub provider: String,
    pub model: String,
    pub effort: Option<String>,
    pub billing: String,
    pub runs: usize,
    pub applied: usize,
    pub failed: usize,
    pub average_duration_ms: Option<u64>,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextAssistAppUsage {
    /// Lower-case executable name; `None` when the target app is unknown.
    pub target_app: Option<String>,
    pub runs: usize,
    pub applied: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextAssistPresetUsage {
    pub kind: String,
    pub preset_id: Option<String>,
    pub runs: usize,
    pub applied: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasonCount {
    /// Sanitized history reason code ("unknown" when none was stored).
    pub code: String,
    pub count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextAssistInsights {
    pub outcomes: TextAssistOutcomeCounts,
    /// Runs that returned a result: applied, kept or no change.
    pub returned_results: usize,
    /// `applied / returned_results`, `None` without results.
    pub apply_rate: Option<f64>,
    /// Over runs that returned a result.
    pub duration: TimingSummary,
    pub first_token: TimingSummary,
    /// Runs that captured text, by provider/model/effort/billing; most runs first.
    pub models: Vec<TextAssistModelUsage>,
    /// Most runs first, at most 12.
    pub apps: Vec<TextAssistAppUsage>,
    pub presets: Vec<TextAssistPresetUsage>,
    /// Reasons of failed runs, most frequent first.
    pub failure_reasons: Vec<ReasonCount>,
    /// Reasons a result was kept in Vellora instead of pasted, most frequent first.
    pub kept_reasons: Vec<ReasonCount>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePeriod {
    pub dictation: DictationUsage,
    pub text_assist: TextAssistUsage,
    pub insights: TextAssistInsights,
}

/// Periods in local time: today, the last 7 and 30 days (including today) and all time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePeriods {
    pub today: UsagePeriod,
    pub last7_days: UsagePeriod,
    pub last30_days: UsagePeriod,
    pub all_time: UsagePeriod,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDay {
    pub date: String,
    pub day_label: String,
    pub transcripts: usize,
    pub dictated_words: usize,
    pub dictation_cost_usd: f64,
    pub text_assist_runs: usize,
    pub text_assist_applied: usize,
    pub words_corrected: usize,
    pub text_assist_cost_usd: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageOverview {
    /// Local date the periods are relative to (YYYY-MM-DD).
    pub today: String,
    pub periods: UsagePeriods,
    /// The last 30 days, oldest first, including days without activity.
    pub daily: Vec<UsageDay>,
    pub assumptions: UsageAssumptions,
    pub dictation_cost_note: String,
    /// Transcripts whose audio length was estimated from the word count.
    pub dictation_estimated_from_words_count: usize,
    /// Transcripts whose cleanup model has no known price.
    pub dictation_unknown_cleanup_model_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppHealthSnapshot {
    pub ready: bool,
    pub startup_phase: StartupPhase,
    pub last_error: Option<DiagnosticEvent>,
    pub active_operation: Option<String>,
    pub backend_started_at: String,
    pub single_instance: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupPhase {
    Booting,
    Migrating,
    InitializingRuntime,
    InitializingTray,
    InitializingOverlay,
    InitializingHotkey,
    Ready,
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEvent {
    pub id: String,
    pub created_at: String,
    pub code: DiagnosticCode,
    pub title: String,
    pub message: String,
    pub detail: Option<String>,
    pub recoverable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    MissingApiKey,
    InvalidApiKey,
    RateLimited,
    NetworkUnavailable,
    NetworkTimeout,
    OpenAiServerError,
    OpenAiBadResponse,
    MicrophoneUnavailable,
    RecordingTooShort,
    ClipboardFailed,
    PasteFailed,
    HistoryWriteFailed,
    StartupNotReady,
    HotkeyStateMismatch,
    Unknown,
}

#[cfg(test)]
mod appearance_tests {
    use super::{AppSettings, Appearance};

    #[test]
    fn existing_settings_without_appearance_keep_notebook() {
        let mut json = serde_json::to_value(AppSettings::default()).unwrap();
        json.as_object_mut().unwrap().remove("appearance");
        let loaded: AppSettings = serde_json::from_value(json).unwrap();
        assert_eq!(loaded.appearance, Appearance::Notebook);
    }

    #[test]
    fn classic_appearance_survives_settings_roundtrip() {
        let settings = AppSettings {
            appearance: Appearance::Classic,
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let loaded: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.appearance, Appearance::Classic);
    }
}

#[cfg(test)]
mod text_assist_settings_tests {
    use super::*;

    /// A settings file as written by the current release: no `textAssist` key.
    const RELEASE_SETTINGS_JSON: &str = r#"{
  "appearance": "notebook",
  "provider": "openai",
  "sttModel": "gpt-4o-mini-transcribe",
  "cleanupEnabled": true,
  "cleanupMode": "light",
  "cleanupModel": "gpt-6-luna",
  "autoPaste": true,
  "copyToClipboard": false,
  "saveHistory": false,
  "language": "de",
  "hotkey": "Ctrl+Win"
}"#;

    #[test]
    fn release_settings_without_text_assist_load_with_defaults() {
        let loaded: AppSettings = serde_json::from_str(RELEASE_SETTINGS_JSON).unwrap();
        assert_eq!(loaded.text_assist, TextAssistSettings::default());
        assert_eq!(loaded.language, "de");
        assert!(!loaded.save_history);
        assert!(!loaded.copy_to_clipboard);
    }

    #[test]
    fn defaults_match_the_plan() {
        let settings = TextAssistSettings::default();
        assert!(settings.enabled);
        assert_eq!(settings.chord, "Shift+Win");
        assert_eq!(settings.hold_threshold_ms, 350);
        assert_eq!(
            settings.quick_model,
            ModelRef::new(ProviderKind::Openai, "gpt-5.6-luna", Some(Effort::None))
        );
        let ids: Vec<_> = settings.presets.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            ["polish", "clearer", "academic", "professional", "custom"]
        );
        assert!(settings.presets.iter().all(|p| p.builtin));
        assert_eq!(settings.last_preset_id, "polish");
        assert_eq!(
            settings.favorite_models,
            vec![
                settings.quick_model.clone(),
                ModelRef::new(ProviderKind::Openai, "gpt-6-luna", Some(Effort::Low)),
            ]
        );
        assert!(settings.ensemble_profiles.is_empty());
        assert!(settings.model_colors.is_empty());
        assert!(settings.save_history);
        assert_eq!(settings.limits.quick_max_chars, 8_000);
        assert_eq!(settings.limits.panel_max_chars, 30_000);
        assert_eq!(settings.limits.ensemble_max_usd_per_run, 0.05);
        assert_eq!(settings.limits.paste_restore_delay_ms, 400);
        assert!(!settings.cli.codex.enabled && settings.cli.codex.path.is_none());
        assert!(!settings.cli.claude.enabled && settings.cli.claude.path.is_none());
        assert!(settings.openai_compatible.is_empty());
        assert!(settings.custom_instructions.is_empty());
        assert!(!settings.dev_offline_transform);
    }

    #[test]
    fn partial_text_assist_block_fills_missing_fields_from_defaults() {
        let mut json: serde_json::Value = serde_json::from_str(RELEASE_SETTINGS_JSON).unwrap();
        json["textAssist"] = serde_json::json!({
            "enabled": false,
            "holdThresholdMs": 500,
            "limits": { "quickMaxChars": 4000 },
            "cli": { "codex": { "enabled": true } },
            "presets": [{ "id": "mine", "name": "Mine", "instruction": "Shorter." }],
            "modelColors": { "openai/gpt-6-luna": "#112233" }
        });
        let loaded: AppSettings = serde_json::from_value(json).unwrap();
        let assist = &loaded.text_assist;
        let defaults = TextAssistSettings::default();
        assert!(!assist.enabled);
        assert_eq!(assist.hold_threshold_ms, 500);
        assert_eq!(assist.limits.quick_max_chars, 4000);
        assert_eq!(assist.limits.panel_max_chars, 30_000);
        assert_eq!(assist.limits.paste_restore_delay_ms, 400);
        assert!(assist.cli.codex.enabled);
        assert!(assist.cli.codex.path.is_none());
        assert!(!assist.cli.claude.enabled);
        assert_eq!(assist.presets.len(), 1);
        assert!(!assist.presets[0].builtin);
        assert!(assist.presets[0].model.is_none());
        assert_eq!(assist.model_colors["openai/gpt-6-luna"], "#112233");
        assert_eq!(assist.chord, defaults.chord);
        assert_eq!(assist.quick_model, defaults.quick_model);
        assert_eq!(assist.favorite_models, defaults.favorite_models);
        assert_eq!(assist.last_preset_id, defaults.last_preset_id);
    }

    #[test]
    fn text_assist_uses_camel_case_keys_and_round_trips() {
        let mut settings = AppSettings::default();
        settings.text_assist.custom_instructions = vec!["Make it shorter".to_string()];
        settings.text_assist.openai_compatible.push(CompatEndpoint {
            id: "ollama".to_string(),
            name: "Ollama".to_string(),
            base_url: "http://127.0.0.1:11434/v1".to_string(),
            models: vec!["llama3".to_string()],
        });
        let json = serde_json::to_value(&settings).unwrap();
        let block = &json["textAssist"];
        for key in [
            "enabled",
            "chord",
            "holdThresholdMs",
            "quickModel",
            "presets",
            "lastPresetId",
            "favoriteModels",
            "ensembleProfiles",
            "modelColors",
            "saveHistory",
            "limits",
            "cli",
            "openaiCompatible",
            "customInstructions",
            "devOfflineTransform",
        ] {
            assert!(block.get(key).is_some(), "missing key {key}");
        }
        assert_eq!(block["limits"]["pasteRestoreDelayMs"], 400);
        assert_eq!(
            block["openaiCompatible"][0]["baseUrl"],
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(block["cli"]["claude"]["path"], serde_json::Value::Null);
        let back: AppSettings = serde_json::from_value(json).unwrap();
        assert_eq!(back.text_assist, settings.text_assist);
    }
}
