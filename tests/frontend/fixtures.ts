import type {
  AppHealthSnapshot,
  AppSettings,
  DashboardStats,
  TextAssistOverview,
  TextAssistRun,
  TextAssistSettings,
  TextModelInfo,
  TranscriptHistoryItem,
} from "../../src/lib/types";

export function settings(overrides: Partial<AppSettings> = {}): AppSettings {
  return {
    appearance: "notebook",
    provider: "openai",
    sttModel: "gpt-4o-mini-transcribe",
    cleanupEnabled: false,
    cleanupMode: "light",
    cleanupModel: "gpt-6-luna",
    autoPaste: true,
    copyToClipboard: true,
    saveHistory: true,
    language: "auto",
    hotkey: "Ctrl+Win",
    textAssist: textAssistSettings(),
    ...overrides,
  };
}

export function textAssistSettings(overrides: Partial<TextAssistSettings> = {}): TextAssistSettings {
  return {
    enabled: true,
    chord: "Shift+Win",
    holdThresholdMs: 350,
    quickModel: { provider: "openai", model: "gpt-6-luna", effort: "none" },
    presets: [],
    lastPresetId: "polish",
    favoriteModels: [{ provider: "openai", model: "gpt-6-luna", effort: "none" }],
    ensembleProfiles: [],
    modelColors: {},
    saveHistory: true,
    limits: { quickMaxChars: 8000, panelMaxChars: 30000, ensembleMaxUsdPerRun: 0.05, pasteRestoreDelayMs: 400 },
    cli: { codex: { enabled: false, path: null }, claude: { enabled: false, path: null } },
    openaiCompatible: [],
    customInstructions: [],
    devOfflineTransform: false,
    ...overrides,
  };
}

export function textModel(overrides: Partial<TextModelInfo> = {}): TextModelInfo {
  return {
    provider: "openai",
    id: "gpt-6-luna",
    label: "GPT-6 Luna",
    efforts: ["none", "low", "medium"],
    defaultEffort: "none",
    inputUsdPerMillion: 0.2,
    outputUsdPerMillion: 0.8,
    color: "#5b8def",
    quickCandidate: true,
    shutdown: null,
    ...overrides,
  };
}

export function textAssistRun(overrides: Partial<TextAssistRun> = {}): TextAssistRun {
  return {
    id: "run-1",
    createdAt: "2026-11-17T10:00:00Z",
    kind: "polish",
    presetId: null,
    ensembleRunId: null,
    provider: "openai",
    model: "gpt-6-luna",
    effort: "none",
    billing: "api",
    originalText: "Teh quick brown fox jump.",
    resultText: "The quick brown fox jumps.",
    outcome: "applied",
    reasonCode: null,
    targetApp: "notepad.exe",
    durationMs: 1200,
    firstTokenMs: 400,
    inputTokens: 120,
    outputTokens: 20,
    estimatedCostUsd: 0.00004,
    ...overrides,
  };
}

export function textAssistOverview(overrides: Partial<TextAssistOverview> = {}): TextAssistOverview {
  return {
    totals: { runs: 0, appliedRuns: 0, estimatedCostUsd: 0, estimatedCostTodayUsd: 0, planRuns: 0 },
    models: [
      textModel({ id: "gpt-5.6-luna", label: "GPT-5.6 Luna", efforts: ["low", "medium", "high"], defaultEffort: "low", quickCandidate: false, inputUsdPerMillion: 1, outputUsdPerMillion: 4 }),
      textModel(),
      textModel({ provider: "anthropic", id: "claude-haiku-5-5", label: "Claude Haiku 5.5", efforts: ["none"] }),
    ],
    lastKept: null,
    ...overrides,
  };
}

export function transcript(overrides: Partial<TranscriptHistoryItem> = {}): TranscriptHistoryItem {
  return {
    id: "test-transcript",
    createdAt: "2026-10-02T10:00:00Z",
    provider: "openai",
    sttModel: "gpt-4o-mini-transcribe",
    cleanupMode: "off",
    rawText: "A recoverable dictation.",
    finalText: "A recoverable dictation.",
    wordCount: 3,
    charCount: 24,
    pasted: false,
    ...overrides,
  };
}

export function health(): AppHealthSnapshot {
  return {
    ready: true,
    startupPhase: "ready",
    backendStartedAt: "2026-10-02T09:00:00Z",
    singleInstance: true,
  };
}

export function stats(): DashboardStats {
  return {
    totalWords: 0,
    totalTranscripts: 0,
    transcriptsToday: 0,
    wordsToday: 0,
    estimatedMinutesSaved: 0,
    averageWordsPerTranscript: 0,
    week: [],
    modelUsage: [],
    cost: {
      pricingVersion: "test-fixture",
      estimatedTotalUsd: 0,
      estimatedTodayUsd: 0,
      estimatedWeekUsd: 0,
      estimatedTranscriptionUsd: 0,
      estimatedCleanupUsd: 0,
      billableAudioSeconds: 0,
      measuredAudioSeconds: 0,
      estimatedAudioSeconds: 0,
      estimatedFromWordsCount: 0,
      unknownModelCount: 0,
      unknownCleanupModelCount: 0,
      week: [],
      modelBreakdown: [],
      note: "Estimates are not official billing.",
    },
  };
}
