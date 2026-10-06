import type { AppHealthSnapshot, AppSettings, DashboardStats, TranscriptHistoryItem } from "../../src/lib/types";

export function settings(overrides: Partial<AppSettings> = {}): AppSettings {
  return {
    appearance: "notebook",
    provider: "openai",
    sttModel: "gpt-4o-mini-transcribe",
    cleanupEnabled: false,
    cleanupMode: "light",
    cleanupModel: "gpt-5-nano-2025-08-07",
    autoPaste: true,
    copyToClipboard: true,
    saveHistory: true,
    language: "auto",
    hotkey: "Ctrl+Win",
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
      week: [],
      modelBreakdown: [],
      note: "Estimates are not official billing.",
    },
  };
}
