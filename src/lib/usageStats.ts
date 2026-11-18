import { useCallback, useEffect, useRef, useState } from "react";
import { getUsageOverview, onTextAssistRunRecorded } from "./api";
import type { TextAssistInsights, UsageOverview, UsagePeriod, UsagePeriodKey } from "./types";

export const USAGE_PERIODS: { key: UsagePeriodKey; label: string; description: string }[] = [
  { key: "today", label: "Today", description: "today" },
  { key: "last7Days", label: "7 days", description: "in the last 7 days" },
  { key: "last30Days", label: "30 days", description: "in the last 30 days" },
  { key: "allTime", label: "All", description: "in total" },
];

function emptyInsights(): TextAssistInsights {
  return {
    outcomes: { applied: 0, kept: 0, noChange: 0, cancelled: 0, failed: 0, other: 0 },
    returnedResults: 0,
    applyRate: null,
    duration: { count: 0, averageMs: null, medianMs: null },
    firstToken: { count: 0, averageMs: null, medianMs: null },
    models: [],
    apps: [],
    presets: [],
    failureReasons: [],
    keptReasons: [],
  };
}

export function emptyUsagePeriod(): UsagePeriod {
  return {
    dictation: { transcripts: 0, words: 0, minutesSaved: 0, estimatedCostUsd: 0 },
    textAssist: {
      runs: 0, applied: 0, failed: 0, wordsProcessed: 0, wordsCorrected: 0, minutesSaved: 0,
      estimatedCostUsd: 0, planRuns: 0, ensembleRuns: 0,
    },
    insights: emptyInsights(),
  };
}

/** An overview without any activity, e.g. for a fresh installation. */
export function emptyUsageOverview(today = "2026-11-18"): UsageOverview {
  const end = new Date(`${today}T12:00:00`);
  const daily = Array.from({ length: 30 }, (_, index) => {
    const date = new Date(end);
    date.setDate(end.getDate() - (29 - index));
    const iso = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
    return {
      date: iso,
      dayLabel: date.toLocaleDateString("en-US", { weekday: "short" }),
      transcripts: 0, dictatedWords: 0, dictationCostUsd: 0,
      textAssistRuns: 0, textAssistApplied: 0, wordsCorrected: 0, textAssistCostUsd: 0,
    };
  });
  return {
    today,
    periods: { today: emptyUsagePeriod(), last7Days: emptyUsagePeriod(), last30Days: emptyUsagePeriod(), allTime: emptyUsagePeriod() },
    daily,
    assumptions: { dictationWordsPerMinute: 40, secondsPerCorrectedWord: 6 },
    dictationCostNote: "Estimated from OpenAI public pricing and local Vellora history. Not official billing data.",
    dictationEstimatedFromWordsCount: 0,
    dictationUnknownCleanupModelCount: 0,
  };
}

/** Short, plain-language reasons for the dashboard lists. */
const reasonLabels: Record<string, string> = {
  no_selection: "Nothing was selected or the app didn't copy",
  empty_selection: "The selection was empty",
  no_text: "The selection contained no text",
  vscode_empty_selection: "VS Code reported an empty selection",
  vscode_multicursor: "Multiple cursors in VS Code",
  clipboard_busy: "Another app was using the clipboard",
  too_long: "The selection was too long",
  password_field: "Password fields are skipped",
  input_blocked: "Windows blocked simulated input (app may run as administrator)",
  modifiers_held: "Keys were still held down",
  target_changed: "The window or selection changed before pasting",
  focus_changed: "Focus moved to another window",
  selection_changed: "The selection changed before pasting",
  selection_lost: "The selection was lost",
  user_activity: "Input kept coming while it was working",
  terminal: "Terminals are skipped",
  elevated_app: "The app runs as administrator",
  clipboard_unsupported: "The clipboard was too large to save",
  typed: "You typed while it was working",
  paste_failed: "Pasting into the app failed",
  no_target: "No target window was found",
  vellora_window: "Started inside a Vellora window",
  missing_key: "No API key is saved",
  invalid_key: "The provider rejected the API key",
  rate_limited: "Provider rate limit reached",
  network: "The provider could not be reached",
  timeout: "The request timed out",
  server_error: "The provider had a server error",
  rejected: "The provider rejected the request",
  bad_response: "The provider sent an unexpected response",
  provider_refused: "The model declined to answer",
  unsupported: "Model or setting not supported",
  cli_not_installed: "Command-line tool not installed",
  cli_not_signed_in: "Command-line tool not signed in",
  cancelled: "Cancelled",
  internal_error: "Text Assist stopped unexpectedly",
  guard_incomplete: "The model's answer was incomplete",
  guard_empty: "The model returned no text",
  guard_preamble: "The model added commentary",
  guard_wrapper: "The model wrapped the text in quotes or a code block",
  guard_markup: "LaTeX, Markdown or code markup changed",
  guard_line_count: "The number of lines changed",
  guard_whitespace: "Indentation, trailing spaces or tabs changed",
  guard_too_many_changes: "It changed more than spelling and grammar",
  guard_length: "The length changed too much",
  unknown: "No reason was recorded",
};

export function reasonLabel(code: string) {
  const known = reasonLabels[code];
  if (known) return known;
  const words = code.replace(/[_-]+/g, " ").trim();
  return words ? `${words[0].toUpperCase()}${words.slice(1)}` : reasonLabels.unknown;
}

const billingLabels: Record<string, string> = {
  api: "API",
  chatgpt_plan: "ChatGPT plan",
  claude_plan: "Claude plan",
  local: "Local",
};

export function billingLabel(billing: string) {
  return billingLabels[billing] ?? billing.replace(/_/g, " ");
}

/** "0 min", "<1 min", "42 min", "3 h 05 min". */
export function formatMinutesSaved(minutes: number) {
  if (!Number.isFinite(minutes) || minutes <= 0) return "0 min";
  if (minutes < 1) return "<1 min";
  const rounded = Math.round(minutes);
  if (rounded < 60) return `${rounded} min`;
  const hours = Math.floor(rounded / 60);
  return `${hours.toLocaleString()} h ${String(rounded % 60).padStart(2, "0")} min`;
}

export function formatRate(rate: number | null) {
  if (rate === null || !Number.isFinite(rate)) return "n/a";
  return `${Math.round(rate * 100)}%`;
}

/** Usage overview, refreshed when `refreshKey` changes and when a Text Assist run is recorded. */
export function useUsageOverview(refreshKey?: unknown) {
  const [overview, setOverview] = useState<UsageOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const requestId = useRef(0);

  const load = useCallback(async () => {
    const id = ++requestId.current;
    try {
      const value = await getUsageOverview();
      if (id === requestId.current) { setOverview(value); setError(null); }
    } catch (reason) {
      if (id === requestId.current) setError(String(reason));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  useEffect(() => {
    let unlisten: Promise<(() => void) | undefined>;
    try {
      unlisten = Promise.resolve(onTextAssistRunRecorded(() => { void load(); }));
    } catch (reason) {
      unlisten = Promise.reject(reason);
    }
    return () => {
      // Invalidate in-flight reads so they cannot update an unmounted view.
      requestId.current += 1;
      unlisten.then((stop) => stop?.()).catch(console.error);
    };
  }, [load]);

  return { overview, error, reload: load };
}
