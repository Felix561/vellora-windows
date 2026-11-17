// Types, command wrappers and pure helpers for the Text Assist Rewrite panel (`assist-panel`).
import { invoke } from "@tauri-apps/api/core";
import type { Appearance } from "./types";

export const PANEL_WINDOW_LABEL = "assist-panel";
export const PANEL_OPEN_EVENT = "text-assist-panel-open";
export const PANEL_RESULT_EVENT = "text-assist-panel";

/** Id of the built-in Custom… preset (Rust `prompts::PRESET_CUSTOM`). */
export const CUSTOM_PRESET_ID = "custom";
/** Number keys 1–9 select presets; later presets are not shown. */
export const MAX_PANEL_PRESETS = 9;
/** Number keys 1–9 select ensemble cards (Rust `ENSEMBLE_MAX_MEMBERS`) and preview profiles. */
export const MAX_ENSEMBLE_MEMBERS = 9;
export const MAX_PANEL_PROFILES = 9;
/** Matches the settings limit for remembered custom instructions. */
export const CUSTOM_INSTRUCTION_MAX_CHARS = 500;
export const CUSTOM_INSTRUCTION_HISTORY = 5;

export type PanelBilling = "api" | "chatgpt_plan" | "claude_plan" | "local";
export type PanelPhase = "thinking" | "streaming" | "done" | "failed" | "cancelled";

export type PanelPreset = { id: string; name: string; instruction: string; builtin: boolean };

export type PanelModel = {
  key: string;
  label: string;
  effort: string | null;
  color: string;
  billing: PanelBilling | (string & {});
};

export type PanelInit = {
  sessionId: string;
  original: string;
  targetApp: string | null;
  appearance: Appearance;
  presets: PanelPreset[];
  activePresetId: string;
  models: PanelModel[];
  activeModelIndex: number;
  customInstructions: string[];
  maxChars: number;
  /** Ensemble profiles; the backend sends an implicit "Favorites" profile when none are configured. */
  ensembleProfiles: PanelEnsembleProfile[];
  /** Per-run cost limit for ensembles in USD. */
  ensembleMaxUsd: number;
};

export type PanelEnsembleProfile = { id: string; name: string; members: PanelModel[] };

/** Shown before an ensemble run (Rust `EnsemblePreview`). */
export type EnsemblePreview = {
  profileId: string;
  members: PanelModel[];
  providers: string[];
  estimatedUsd: number;
  /** A paid member has no catalog price; the estimate leaves it out. */
  unknownCost: boolean;
  limitUsd: number;
  blocked: boolean;
};

export type EnsembleArgs = {
  profileId: string;
  presetId: string;
  customInstruction: string | null;
};

export type PanelResult = {
  sessionId: string;
  requestId: string;
  presetId: string;
  modelIndex: number;
  phase: PanelPhase;
  /** Accumulated output; the final text when done. */
  text: string;
  /** Model label. */
  label: string;
  effort: string | null;
  billing: PanelBilling | (string & {});
  color: string;
  durationMs: number | null;
  costUsd: number | null;
  /** Guard reason codes such as `guard_markup`. */
  warnings: string[];
  /** Short error label. */
  error: string | null;
  /** Ensemble members only: the run they belong to. */
  ensembleRunId?: string;
  /** Ensemble members only: position in the profile (`modelIndex` is then meaningless). */
  memberIndex?: number;
};

export type PanelRunArgs = {
  presetId: string;
  modelIndex: number;
  customInstruction: string | null;
  fresh: boolean;
};

// Commands (Tauri converts the camelCase argument names).

export function getPanelInit() {
  return invoke<PanelInit | null>("get_panel_init");
}

/** Starts (or, with `fresh: false`, replays a cached) run; resolves to its request id. */
export function panelRun(args: PanelRunArgs) {
  return invoke<string>("panel_run", args);
}

export function panelApply(requestId: string) {
  return invoke<void>("panel_apply", { requestId });
}

export function panelCopy(requestId: string) {
  return invoke<void>("panel_copy", { requestId });
}

export function panelEnsemblePreview(args: EnsembleArgs) {
  return invoke<unknown>("panel_ensemble_preview", args);
}

/** Starts every member of a profile; resolves to the ensemble run id. Rejects above the cost limit. */
export function panelRunEnsemble(args: EnsembleArgs) {
  return invoke<string>("panel_run_ensemble", args);
}

export function panelClose() {
  return invoke<void>("panel_close");
}

// Pure helpers.

const PHASES: readonly PanelPhase[] = ["thinking", "streaming", "done", "failed", "cancelled"];
const COLOR_PATTERN = /^#[0-9a-fA-F]{6}$/;
export const PANEL_FALLBACK_COLOR = "#8a8a8a";

/** Cache key for one preset + model combination. */
export function resultKey(presetId: string, modelIndex: number) {
  return `${presetId}|${modelIndex}`;
}

export function isCustomPreset(presetId: string) {
  return presetId === CUSTOM_PRESET_ID;
}

export function safeColor(value: unknown): string {
  return typeof value === "string" && COLOR_PATTERN.test(value) ? value : PANEL_FALLBACK_COLOR;
}

const billingLabels: Record<string, string> = {
  api: "API",
  chatgpt_plan: "ChatGPT plan",
  claude_plan: "Claude plan",
  local: "Local",
};

export function billingLabel(billing: string | null | undefined) {
  if (!billing) return "";
  return billingLabels[billing] ?? billing;
}

const warningTexts: Record<string, string> = {
  guard_markup: "LaTeX/Markdown/code may have changed",
  guard_too_many_changes: "Large change",
  guard_length: "Length changed a lot",
  guard_line_count: "Line breaks changed",
  guard_preamble: "Model added extra text",
  guard_wrapper: "Model added extra text",
  guard_incomplete: "Output incomplete",
};

export function warningText(code: string) {
  return warningTexts[code] ?? "Check this result";
}

/** Friendly warning texts, without duplicates (preamble and wrapper share one text). */
export function friendlyWarnings(codes: readonly string[]) {
  return [...new Set(codes.map(warningText))];
}

/** "1.4 s", "820 ms". */
export function formatPanelDuration(ms: number | null) {
  if (ms === null || !Number.isFinite(ms) || ms < 0) return null;
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(1)} s`;
}

/** "≈ $0.0004" for API runs with a known cost; null otherwise. */
export function formatPanelCost(costUsd: number | null) {
  if (costUsd === null || !Number.isFinite(costUsd) || costUsd <= 0) return null;
  return costUsd < 0.01 ? `≈ $${costUsd.toFixed(4)}` : `≈ $${costUsd.toFixed(2)}`;
}

/** "texstudio.exe" → "texstudio". */
export function targetAppName(targetApp: string | null) {
  if (!targetApp) return null;
  const name = targetApp.replace(/\.exe$/i, "").trim();
  return name || null;
}

/** Wraps around in both directions; -1 when there are no models. */
export function nextModelIndex(current: number, count: number, delta: 1 | -1) {
  if (count <= 0) return -1;
  return (((current + delta) % count) + count) % count;
}

/** "1–5" for the footer hint. */
export function presetKeyRange(count: number) {
  const shown = Math.min(count, MAX_PANEL_PRESETS);
  if (shown <= 1) return "1";
  return `1–${shown}`;
}

/** Prefers a newly typed instruction at the front; keeps at most five, without duplicates. */
export function rememberInstruction(history: readonly string[], instruction: string) {
  const trimmed = instruction.trim();
  if (!trimmed) return [...history];
  return [trimmed, ...history.filter((item) => item !== trimmed)].slice(0, CUSTOM_INSTRUCTION_HISTORY);
}

function str(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function num(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function normalizePanelModels(raw: unknown): PanelModel[] {
  return (Array.isArray(raw) ? raw : [])
    .filter((item): item is Record<string, unknown> => !!item && typeof item === "object")
    .map((item) => ({
      key: str(item.key) ?? "",
      label: str(item.label) ?? "Model",
      effort: str(item.effort),
      color: safeColor(item.color),
      billing: str(item.billing) ?? "",
    }));
}

function normalizeProfiles(raw: unknown): PanelEnsembleProfile[] {
  return (Array.isArray(raw) ? raw : [])
    .filter((item): item is Record<string, unknown> => !!item && typeof item === "object")
    .filter((item) => typeof item.id === "string" && item.id.length > 0)
    .map((item) => ({
      id: item.id as string,
      name: str(item.name) || (item.id as string),
      members: normalizePanelModels(item.members).slice(0, MAX_ENSEMBLE_MEMBERS),
    }))
    .filter((profile) => profile.members.length > 0)
    .slice(0, MAX_PANEL_PROFILES);
}

/** Validates an untrusted init payload; null when it is unusable. */
export function normalizePanelInit(raw: unknown): PanelInit | null {
  if (!raw || typeof raw !== "object") return null;
  const p = raw as Record<string, unknown>;
  const sessionId = str(p.sessionId);
  if (!sessionId) return null;
  const presets = (Array.isArray(p.presets) ? p.presets : [])
    .filter((item): item is Record<string, unknown> => !!item && typeof item === "object")
    .filter((item) => typeof item.id === "string" && typeof item.name === "string")
    .slice(0, MAX_PANEL_PRESETS)
    .map((item) => ({
      id: item.id as string,
      name: item.name as string,
      instruction: str(item.instruction) ?? "",
      builtin: item.builtin === true,
    }));
  const models = normalizePanelModels(p.models);
  const activePresetId = presets.some((preset) => preset.id === p.activePresetId)
    ? (p.activePresetId as string)
    : presets[0]?.id ?? "";
  const requestedModel = num(p.activeModelIndex) ?? 0;
  const activeModelIndex = models.length === 0
    ? 0
    : Math.min(models.length - 1, Math.max(0, Math.trunc(requestedModel)));
  return {
    sessionId,
    original: str(p.original) ?? "",
    targetApp: str(p.targetApp),
    appearance: p.appearance === "classic" ? "classic" : "notebook",
    presets,
    activePresetId,
    models,
    activeModelIndex,
    customInstructions: (Array.isArray(p.customInstructions) ? p.customInstructions : [])
      .filter((item): item is string => typeof item === "string" && item.trim().length > 0)
      .slice(0, CUSTOM_INSTRUCTION_HISTORY),
    maxChars: num(p.maxChars) ?? 0,
    ensembleProfiles: normalizeProfiles(p.ensembleProfiles),
    ensembleMaxUsd: Math.max(0, num(p.ensembleMaxUsd) ?? 0),
  };
}

/** Validates an untrusted result event; null when it is unusable. */
export function normalizePanelResult(raw: unknown): PanelResult | null {
  if (!raw || typeof raw !== "object") return null;
  const p = raw as Record<string, unknown>;
  const sessionId = str(p.sessionId);
  const requestId = str(p.requestId);
  const presetId = str(p.presetId);
  const modelIndex = num(p.modelIndex);
  if (!sessionId || !requestId || presetId === null || modelIndex === null) return null;
  if (!PHASES.includes(p.phase as PanelPhase)) return null;
  return {
    sessionId,
    requestId,
    presetId,
    modelIndex,
    phase: p.phase as PanelPhase,
    text: str(p.text) ?? "",
    label: str(p.label) ?? "",
    effort: str(p.effort),
    billing: str(p.billing) ?? "",
    color: safeColor(p.color),
    durationMs: num(p.durationMs),
    costUsd: num(p.costUsd),
    warnings: (Array.isArray(p.warnings) ? p.warnings : []).filter((item): item is string => typeof item === "string"),
    error: str(p.error),
    ...ensembleFields(p),
  };
}

function ensembleFields(p: Record<string, unknown>): Pick<PanelResult, "ensembleRunId" | "memberIndex"> {
  const runId = str(p.ensembleRunId);
  const member = num(p.memberIndex);
  if (!runId || member === null || member < 0 || !Number.isInteger(member)) return {};
  return { ensembleRunId: runId, memberIndex: member };
}

/** Validates an untrusted preview; null when it is unusable. */
export function normalizeEnsemblePreview(raw: unknown): EnsemblePreview | null {
  if (!raw || typeof raw !== "object") return null;
  const p = raw as Record<string, unknown>;
  const profileId = str(p.profileId);
  if (!profileId) return null;
  const estimatedUsd = num(p.estimatedUsd);
  const limitUsd = num(p.limitUsd);
  return {
    profileId,
    members: normalizePanelModels(p.members).slice(0, MAX_ENSEMBLE_MEMBERS),
    providers: (Array.isArray(p.providers) ? p.providers : []).filter((item): item is string => typeof item === "string"),
    estimatedUsd: estimatedUsd !== null && estimatedUsd > 0 ? estimatedUsd : 0,
    unknownCost: p.unknownCost === true,
    limitUsd: limitUsd !== null && limitUsd > 0 ? limitUsd : 0,
    // Fail closed: anything but an explicit `false` counts as blocked.
    blocked: p.blocked !== false,
  };
}

/** Estimates and limits: "$0.0123", "$0.05", "$1.20"; "$0" for nothing. */
export function formatUsd(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "$0";
  if (value < 0.0001) return "<$0.0001";
  if (value >= 1) return `$${value.toFixed(2)}`;
  // Four decimals, without trailing zeros beyond cents.
  return `$${value.toFixed(4).replace(/0{1,2}$/, "")}`;
}

/** "1–3" for the card footer hint. */
export function cardKeyRange(count: number) {
  const shown = Math.min(count, MAX_ENSEMBLE_MEMBERS);
  return shown <= 1 ? "1" : `1–${shown}`;
}

const providerNames: Record<string, string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  gemini: "Google Gemini",
  openai_compatible: "OpenAI-compatible",
  codex_cli: "ChatGPT plan (Codex CLI)",
  claude_cli: "Claude plan (Claude Code)",
  offline: "Offline test",
};

export function providerName(provider: string) {
  return providerNames[provider] ?? provider;
}

/** Apply needs a finished result with text. */
export function canApply(result: PanelResult | null | undefined) {
  return !!result && result.phase === "done" && result.text.trim().length > 0;
}
