import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AppSnapshot,
  AppHealthSnapshot,
  AudioLevelEvent,
  DashboardStats,
  DiagnosticEvent,
  DictationState,
  AppSettings,
  OverlaySnapshot,
  TranscriptHistoryItem,
  HelpLinkId,
  SupportInfo,
  MicrophoneCheck,
} from "./types";

export function getSettings() {
  return invoke<AppSettings>("get_settings");
}

export function saveSettings(settings: AppSettings) {
  return invoke<AppSettings>("save_settings", { settings });
}

export function hasOpenAiApiKey() {
  return invoke<boolean>("has_openai_api_key");
}

export function saveOpenAiApiKey(key: string) {
  return invoke<void>("save_openai_api_key", { key });
}

export function deleteOpenAiApiKey() {
  return invoke<void>("delete_openai_api_key");
}

export function getDictationSnapshot() {
  return invoke<AppSnapshot>("get_dictation_snapshot");
}

export function listTranscripts(limit = 50) {
  return invoke<TranscriptHistoryItem[]>("list_transcripts", { limit });
}

export function copyTranscript(id: string) {
  return invoke<void>("copy_transcript", { id });
}

export function pasteTranscript(id: string) {
  return invoke<void>("paste_transcript", { id });
}

export function deleteTranscript(id: string) {
  return invoke<boolean>("delete_transcript", { id });
}

export function clearTranscripts() {
  return invoke<number>("clear_transcripts");
}

export function onHistoryChanged(handler: () => void) {
  return listen("history-changed", handler);
}

export function getDashboardStats() {
  return invoke<DashboardStats>("get_dashboard_stats");
}

export function getAppHealth() {
  return invoke<AppHealthSnapshot>("get_app_health");
}

export function showMainWindow() {
  return invoke<void>("show_main_window_command");
}

export function clearError() {
  return invoke<void>("clear_error");
}

export function onDictationStateChanged(handler: (state: DictationState) => void) {
  return listen<DictationState>("dictation-state-changed", (event) => handler(event.payload));
}

export function onTranscriptCreated(handler: (item: TranscriptHistoryItem) => void) {
  return listen<TranscriptHistoryItem>("transcript-created", (event) => handler(event.payload));
}

export function onDictationError(handler: (message: string) => void) {
  return listen<string>("dictation-error", (event) => handler(event.payload));
}

export function onDiagnosticEvent(handler: (event: DiagnosticEvent) => void) {
  return listen<DiagnosticEvent>("diagnostic-event", (event) => handler(event.payload));
}

export function onAppHealthChanged(handler: (event: AppHealthSnapshot) => void) {
  return listen<AppHealthSnapshot>("app-health-changed", (event) => handler(event.payload));
}

export function onOpenDashboardRequested(handler: () => void) {
  return listen("open-dashboard-requested", () => handler());
}

export function onAudioLevel(handler: (event: AudioLevelEvent) => void) {
  return listen<AudioLevelEvent>("audio-level", (event) => handler(event.payload));
}

export function onOverlayStateChanged(handler: (event: OverlaySnapshot) => void) {
  return listen<OverlaySnapshot>("overlay-state-changed", (event) => handler(event.payload));
}

export function getStartAtLogin() {
  return invoke<boolean>("get_start_at_login");
}

export function setStartAtLogin(enabled: boolean) {
  return invoke<boolean>("set_start_at_login", { enabled });
}

export function onSettingsChanged(handler: (settings: AppSettings) => void) {
  return listen<AppSettings>("settings-changed", (event) => handler(event.payload));
}

export function getSupportInfo() {
  return invoke<SupportInfo>("get_support_info");
}

export function checkMicrophone() {
  return invoke<MicrophoneCheck>("check_microphone");
}

export function openHelpLink(id: HelpLinkId) {
  return invoke<void>("open_help_link", { id });
}
