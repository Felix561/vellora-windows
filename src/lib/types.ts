export type Provider = "openai";

export type SttModel =
  | "whisper-1"
  | "gpt-4o-mini-transcribe"
  | "gpt-4o-transcribe"
  | "gpt-4o-transcribe-diarize";

export type CleanupMode = "off" | "light";

export type DictationState =
  | "idle"
  | "recording"
  | "transcribing"
  | "cleaning"
  | "pasting"
  | "done"
  | "error";

export type RecordingMode = "hold" | "locked";

export type Appearance = "notebook" | "classic";

export type AppSettings = {
  appearance: Appearance;
  provider: Provider;
  sttModel: SttModel;
  cleanupEnabled: boolean;
  cleanupMode: "light";
  cleanupModel: "gpt-5-nano-2025-08-07";
  autoPaste: boolean;
  copyToClipboard: boolean;
  saveHistory: boolean;
  language: "auto";
  hotkey: "Ctrl+Win";
};

/** Submit changed fields; App merges and serializes writes against confirmed settings. */
export type SettingsChange = (patch: Partial<AppSettings>) => Promise<void>;

export type TranscriptHistoryItem = {
  id: string;
  createdAt: string;
  provider: Provider;
  sttModel: SttModel;
  cleanupModel?: string | null;
  cleanupMode: CleanupMode;
  rawText: string;
  finalText: string;
  durationMs?: number | null;
  wordCount: number;
  charCount: number;
  pasted: boolean;
  error?: string | null;
  pipelineStartedAt?: string | null;
  recordingDurationMs?: number | null;
  transcriptionDurationMs?: number | null;
  cleanupDurationMs?: number | null;
  pasteDurationMs?: number | null;
  totalPipelineDurationMs?: number | null;
};

export type AppSnapshot = {
  state: DictationState;
  message?: string | null;
  lastTranscript?: TranscriptHistoryItem | null;
  recordingMode?: RecordingMode | null;
};

export type DailyStat = {
  date: string;
  dayLabel: string;
  transcriptCount: number;
  wordCount: number;
};

export type ModelUsageStat = {
  model: string;
  transcriptCount: number;
  averageDurationMs?: number | null;
};

export type DashboardStats = {
  totalWords: number;
  totalTranscripts: number;
  transcriptsToday: number;
  wordsToday: number;
  estimatedMinutesSaved: number;
  averageWordsPerTranscript: number;
  mostActiveDay?: string | null;
  week: DailyStat[];
  modelUsage: ModelUsageStat[];
  cost: CostSummary;
};

export type AudioLevelEvent = {
  level: number;
};

export type OverlaySnapshot = {
  state: DictationState;
  message?: string | null;
  level?: number | null;
  model?: string | null;
  cleanupEnabled?: boolean | null;
  lastTranscriptPreview?: string | null;
  recordingMode?: RecordingMode | null;
};

export type StartupPhase =
  | "booting"
  | "migrating"
  | "initializingRuntime"
  | "initializingTray"
  | "initializingOverlay"
  | "initializingHotkey"
  | "ready"
  | "degraded";

export type DiagnosticEvent = {
  id: string;
  createdAt: string;
  code: string;
  title: string;
  message: string;
  detail?: string | null;
  recoverable: boolean;
};

export type AppHealthSnapshot = {
  ready: boolean;
  startupPhase: StartupPhase;
  lastError?: DiagnosticEvent | null;
  activeOperation?: string | null;
  backendStartedAt: string;
  singleInstance: boolean;
};

export type HelpLinkId =
  | "repository"
  | "readme"
  | "troubleshooting"
  | "privacy"
  | "license"
  | "openaiKeys"
  | "openaiBilling"
  | "windowsMicrophone";

/** Support metadata deliberately excludes device names, paths, errors and transcript text. */
export type SupportInfo = {
  version: string;
  platform: "windows";
  architecture: string;
  startupPhase: StartupPhase;
  ready: boolean;
  singleInstance: boolean;
  keyConfigured: boolean;
  microphoneAvailable: boolean;
  dictationState: DictationState;
};

export type MicrophoneCheck = {
  available: boolean;
  deviceName?: string | null;
};

export type CostSummary = {
  pricingVersion: string;
  estimatedTotalUsd: number;
  estimatedTodayUsd: number;
  estimatedWeekUsd: number;
  estimatedTranscriptionUsd: number;
  estimatedCleanupUsd: number;
  billableAudioSeconds: number;
  measuredAudioSeconds: number;
  estimatedAudioSeconds: number;
  estimatedFromWordsCount: number;
  unknownModelCount: number;
  week: DailyCostStat[];
  modelBreakdown: ModelCostStat[];
  note: string;
};

export type DailyCostStat = {
  date: string;
  dayLabel: string;
  estimatedCostUsd: number;
  transcriptionCount: number;
  audioSeconds: number;
};

export type ModelCostStat = {
  model: string;
  estimatedCostUsd: number;
  transcriptionCount: number;
  audioSeconds: number;
};
