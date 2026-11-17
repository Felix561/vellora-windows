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
  /** OpenAI model ID; defaults to gpt-6-luna. Older saved IDs are migrated on load. */
  cleanupModel: string;
  autoPaste: boolean;
  copyToClipboard: boolean;
  saveHistory: boolean;
  language: "auto";
  hotkey: "Ctrl+Win";
  /** Nested `textAssist` block (serde default on the Rust side). */
  textAssist: TextAssistSettings;
};

/** Rust `ProviderKind` (snake_case). */
export type TextProviderKind =
  | "openai"
  | "anthropic"
  | "gemini"
  | "openai_compatible"
  | "codex_cli"
  | "claude_cli"
  | "offline";

/** Rust `Effort` (lowercase). */
export type Effort = "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max";

/** Rust `ModelRef`; `effort` and `endpoint` are omitted when unset. */
export type ModelRef = {
  provider: TextProviderKind;
  model: string;
  effort?: Effort | null;
  endpoint?: string | null;
};

export type RewritePreset = {
  id: string;
  name: string;
  instruction: string;
  model: ModelRef | null;
  color: string | null;
  builtin: boolean;
};

export type EnsembleProfile = {
  id: string;
  name: string;
  members: ModelRef[];
};

export type TextAssistLimits = {
  quickMaxChars: number;
  panelMaxChars: number;
  ensembleMaxUsdPerRun: number;
  pasteRestoreDelayMs: number;
};

export type CliBridge = {
  enabled: boolean;
  path: string | null;
};

export type CompatEndpoint = {
  id: string;
  name: string;
  baseUrl: string;
  models: string[];
};

export type TextAssistSettings = {
  enabled: boolean;
  chord: string;
  holdThresholdMs: number;
  quickModel: ModelRef;
  presets: RewritePreset[];
  lastPresetId: string;
  favoriteModels: ModelRef[];
  ensembleProfiles: EnsembleProfile[];
  /** Keyed by `provider/model`, e.g. `openai/gpt-6-luna`. */
  modelColors: Record<string, string>;
  saveHistory: boolean;
  limits: TextAssistLimits;
  cli: { codex: CliBridge; claude: CliBridge };
  openaiCompatible: CompatEndpoint[];
  /** Newest first, at most five. */
  customInstructions: string[];
  /** Developer-only offline uppercase engine (no network). */
  devOfflineTransform: boolean;
};

export type TextAssistKind = "polish" | "rewrite" | "ensemble";

export type TextAssistOutcome = "applied" | "no_change" | "kept" | "cancelled" | "failed";

export type TextAssistBilling = "api" | "chatgpt_plan" | "claude_plan" | "local";

/** Rust `history::TextAssistRun`. String fields are kept open for forward compatibility. */
export type TextAssistRun = {
  id: string;
  createdAt: string;
  kind: TextAssistKind | (string & {});
  presetId: string | null;
  ensembleRunId: string | null;
  provider: TextProviderKind | (string & {});
  model: string;
  effort: Effort | (string & {}) | null;
  billing: TextAssistBilling | (string & {});
  originalText: string;
  resultText: string | null;
  outcome: TextAssistOutcome | (string & {});
  /** Sanitized code such as `target_changed` or `guard_markup`. */
  reasonCode: string | null;
  /** Executable file name only. */
  targetApp: string | null;
  durationMs: number | null;
  firstTokenMs: number | null;
  inputTokens: number | null;
  outputTokens: number | null;
  estimatedCostUsd: number | null;
};

export type TextAssistCostTotals = {
  runs: number;
  appliedRuns: number;
  estimatedCostUsd: number;
  estimatedCostTodayUsd: number;
  /** Runs billed to a ChatGPT or Claude plan. */
  planRuns: number;
};

/** A catalog model as shown in the model pickers. */
export type TextModelInfo = {
  provider: TextProviderKind;
  id: string;
  label: string;
  efforts: Effort[];
  defaultEffort: Effort | null;
  inputUsdPerMillion: number;
  outputUsdPerMillion: number;
  color: string;
  quickCandidate: boolean;
  shutdown: string | null;
};

export type TextAssistOverview = {
  totals: TextAssistCostTotals;
  models: TextModelInfo[];
  lastKept: TextAssistRun | null;
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
  unknownCleanupModelCount: number;
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
