import { useCallback, useEffect, useRef, useState } from "react";
import { getTextAssistOverview, onTextAssistRunRecorded } from "./api";
import type { Effort, TextAssistOverview, TextAssistRun, TextModelInfo } from "./types";

export const QUICK_MAX_CHARS = { min: 100, max: 50_000 } as const;
export const PASTE_RESTORE_DELAY_MS = { min: 50, max: 5_000 } as const;

const effortLabels: Record<Effort, string> = {
  none: "None",
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra high",
  max: "Max",
};

export function effortLabel(effort: string | null | undefined) {
  if (!effort) return "default";
  return effortLabels[effort as Effort] ?? effort;
}

const outcomeLabels: Record<string, string> = {
  applied: "Applied",
  no_change: "No change",
  kept: "Kept",
  cancelled: "Cancelled",
  failed: "Failed",
};

export function outcomeLabel(outcome: string) {
  return outcomeLabels[outcome] ?? outcome.replace(/_/g, " ");
}

const kindLabels: Record<string, string> = {
  polish: "Quick Polish",
  rewrite: "Rewrite",
  ensemble: "Ensemble",
};

export function kindLabel(kind: string) {
  return kindLabels[kind] ?? kind;
}

/** Friendly text for the sanitized history reason codes. */
const reasonTexts: Record<string, string> = {
  target_changed: "The target window or selection changed, so nothing was pasted.",
  focus_changed: "Focus moved to another window, so nothing was pasted.",
  user_activity: "You typed or clicked while the result was being applied, so nothing was pasted.",
  typed: "You typed while the result was being applied, so nothing was pasted.",
  modifiers_held: "Keys were still held down, so the paste was skipped.",
  input_blocked: "Windows blocked simulated input. The app may be running as administrator.",
  clipboard_busy: "Another app was using the clipboard.",
  paste_failed: "Pasting into the app failed.",
  no_target: "No target window was found.",
  vellora_window: "Text Assist does not run inside Vellora's own windows.",
  no_selection: "No text was selected.",
  empty_selection: "The selection was empty.",
  no_text: "The selection contained no text.",
  password_field: "Password fields are not supported.",
  too_long: "The selection is longer than the Quick Polish limit.",
  vscode_empty_selection: "VS Code reported an empty selection.",
  vscode_multicursor: "Multiple cursors in VS Code are not supported.",
  guard_incomplete: "The model's answer was incomplete, so it was not pasted.",
  guard_empty: "The model returned no text.",
  guard_preamble: "The model added commentary, so the result was not pasted.",
  guard_wrapper: "The model wrapped the text in quotes or a code block, so it was not pasted.",
  guard_markup: "LaTeX, Markdown or code markup changed, so the result was not pasted.",
  guard_line_count: "The number of lines changed, so the result was not pasted.",
  guard_too_many_changes: "The model changed more than spelling and grammar, so the result was not pasted.",
  guard_length: "The length changed too much, so the result was not pasted.",
  missing_key: "No OpenAI API key is saved.",
  invalid_key: "The provider rejected the API key.",
  rate_limited: "The provider's rate limit was reached. Try again shortly.",
  network: "The provider could not be reached.",
  timeout: "The request timed out.",
  server_error: "The provider reported a server error.",
  rejected: "The provider rejected the request.",
  bad_response: "The provider sent an unexpected response.",
  provider_refused: "The model declined to answer.",
  unsupported: "This model or setting is not supported.",
  cli_not_installed: "The command-line tool is not installed.",
  cli_not_signed_in: "The command-line tool is not signed in.",
  cancelled: "Cancelled.",
  internal_error: "Text Assist stopped unexpectedly.",
};

export function reasonText(code: string | null | undefined) {
  if (!code) return null;
  return reasonTexts[code] ?? `Reason: ${code.replace(/_/g, " ")}.`;
}

/** Price as "$0.20 in / $0.80 out per 1M tokens". */
export function formatModelPrice(model: TextModelInfo) {
  return `$${model.inputUsdPerMillion.toFixed(2)} in / $${model.outputUsdPerMillion.toFixed(2)} out per 1M tokens`;
}

/** Per-run cost; tiny values keep four decimals. */
export function formatRunCost(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "$0";
  if (value < 0.0001) return "<$0.0001";
  if (value < 0.01) return `$${value.toFixed(4)}`;
  return `$${value.toFixed(2)}`;
}

/** Text a "Copy" action should place on the clipboard for a run. */
export function runCopyText(run: TextAssistRun) {
  return run.resultText ?? run.originalText;
}

/** Text Assist overview, refreshed whenever the backend records a new run. */
export function useTextAssistOverview() {
  const [overview, setOverview] = useState<TextAssistOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const requestId = useRef(0);

  const load = useCallback(async () => {
    const id = ++requestId.current;
    try {
      const value = await getTextAssistOverview();
      if (id === requestId.current) { setOverview(value); setError(null); }
    } catch (reason) {
      if (id === requestId.current) setError(String(reason));
    }
  }, []);

  useEffect(() => {
    void load();
    const unlisten = onTextAssistRunRecorded(() => { void load(); });
    return () => {
      // Invalidate in-flight reads so they cannot update an unmounted view.
      requestId.current += 1;
      unlisten.then((stop) => stop()).catch(console.error);
    };
  }, [load]);

  return { overview, error, reload: load };
}
