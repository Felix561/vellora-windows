use serde::{Deserialize, Serialize};

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
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            appearance: Appearance::Notebook,
            provider: Provider::Openai,
            stt_model: SttModel::Whisper1,
            cleanup_enabled: false,
            cleanup_mode: FixedCleanupMode::Light,
            cleanup_model: "gpt-5-nano-2025-08-07".to_string(),
            auto_paste: true,
            copy_to_clipboard: true,
            save_history: true,
            language: "auto".to_string(),
            hotkey: "Ctrl+Win".to_string(),
        }
    }
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
