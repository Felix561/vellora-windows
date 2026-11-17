import { useCallback, useEffect, useRef, useState } from "react";
import { getTextAssistOverview, getTextProviderKeys, onTextAssistRunRecorded } from "./api";
import type {
  CompatEndpoint,
  Effort,
  EnsembleProfile,
  ModelRef,
  TextAssistOverview,
  TextAssistRun,
  TextAssistSettings,
  TextModelInfo,
  TextProviderKeys,
  TextProviderKind,
} from "./types";

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

// ---------------------------------------------------------------------------
// Providers, model references and favorites (Models -> Text models).
// Limits mirror the Rust settings validation (`settings.rs`).
// ---------------------------------------------------------------------------

export const MAX_FAVORITES = 12;
export const MAX_ENDPOINTS = 10;
export const MAX_ENDPOINT_MODELS = 50;
export const MAX_MODEL_COLORS = 64;
export const MAX_ENDPOINT_NAME = 40;

/** Catalog providers in display order. */
export const CATALOG_PROVIDERS = ["openai", "anthropic", "gemini"] as const satisfies readonly TextProviderKind[];

const providerLabels: Record<TextProviderKind, string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  gemini: "Google Gemini",
  openai_compatible: "OpenAI-compatible",
  codex_cli: "ChatGPT plan",
  claude_cli: "Claude plan",
  offline: "Offline",
};

export function providerLabel(provider: string) {
  return providerLabels[provider as TextProviderKind] ?? provider;
}

/** Mirrors Rust `catalog::default_color`. */
export function defaultProviderColor(provider: string) {
  switch (provider) {
    case "openai": return "#10a37f";
    case "anthropic":
    case "claude_cli": return "#d97757";
    case "gemini": return "#4285f4";
    default: return "#8a8f98";
  }
}

/** `[A-Za-z0-9._:/-]{1,128}` */
export function isModelId(value: string) {
  return /^[A-Za-z0-9._:/-]{1,128}$/.test(value);
}

/** `[a-z0-9_-]{1,40}` */
export function isEndpointId(value: string) {
  return /^[a-z0-9_-]{1,40}$/.test(value);
}

/** `#rrggbb` */
export function isHexColor(value: string) {
  return /^#[0-9a-fA-F]{6}$/.test(value);
}

/** Endpoint id derived from its name: lowercase slug, unique among `taken`, at most 40 characters. */
export function endpointIdFromName(name: string, taken: Iterable<string> = []) {
  return idFromName(name, taken, "endpoint");
}

/** Ensemble profile id derived from its name, like endpoint ids. */
export function profileIdFromName(name: string, taken: Iterable<string> = []) {
  return idFromName(name, taken, "profile");
}

function idFromName(name: string, taken: Iterable<string>, fallback: string) {
  const used = new Set(taken);
  const base = name
    .normalize("NFKD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^[-_]+|[-_]+$/g, "")
    .slice(0, 40)
    .replace(/[-_]+$/, "") || fallback;
  if (!used.has(base)) return base;
  for (let suffix = 2; ; suffix += 1) {
    const tail = `-${suffix}`;
    const candidate = `${base.slice(0, 40 - tail.length).replace(/[-_]+$/, "")}${tail}`;
    if (!used.has(candidate)) return candidate;
  }
}

const LOOPBACK_HOSTS = new Set(["127.0.0.1", "localhost", "[::1]"]);

function parseUrl(value: string) {
  try {
    return new URL(value.trim());
  } catch {
    return null;
  }
}

/** Returns a message when `value` is not an allowed endpoint base URL, else null. */
export function validateBaseUrl(value: string) {
  const text = value.trim();
  if (!text) return "Enter the endpoint's base URL, for example http://127.0.0.1:11434/v1.";
  const url = parseUrl(text);
  if (!url || (url.protocol !== "http:" && url.protocol !== "https:")) {
    return "Enter a full URL starting with https://, or http:// for 127.0.0.1, localhost or [::1].";
  }
  if (!url.hostname) return "Enter a full URL including the host name.";
  if (url.username || url.password || url.search || url.hash) {
    return "Remove sign-in details, query strings and # fragments from the URL. Save a key in the key field instead.";
  }
  if (url.protocol === "http:" && !LOOPBACK_HOSTS.has(url.hostname)) {
    return "Use HTTPS. Plain HTTP is only allowed for 127.0.0.1, localhost or [::1].";
  }
  return null;
}

/** True for URLs on this computer (Ollama, LM Studio). */
export function isLocalBaseUrl(value: string) {
  const url = parseUrl(value);
  return Boolean(url && LOOPBACK_HOSTS.has(url.hostname));
}

/** Splits a comma or line separated model list; trims, drops blanks and duplicates. */
export function parseModelList(text: string) {
  const seen = new Set<string>();
  for (const part of text.split(/[\s,;]+/)) {
    const id = part.trim();
    if (id) seen.add(id);
  }
  return [...seen];
}

export type EndpointDraft = { name: string; baseUrl: string; models: string };
export type EndpointErrors = Partial<Record<keyof EndpointDraft, string>>;

/**
 * Validates an endpoint draft against the Rust rules. `editingId` is the id of the endpoint
 * being edited (null when adding). Returns the endpoint to save, or the field errors.
 */
export function validateEndpointDraft(draft: EndpointDraft, existing: readonly CompatEndpoint[], editingId: string | null):
  { endpoint: CompatEndpoint; errors: null } | { endpoint: null; errors: EndpointErrors } {
  const errors: EndpointErrors = {};
  const name = draft.name.trim();
  if (editingId === null && existing.length >= MAX_ENDPOINTS) errors.name = `Vellora supports at most ${MAX_ENDPOINTS} endpoints.`;
  else if (!name) errors.name = "Enter a name.";
  else if ([...name].length > MAX_ENDPOINT_NAME) errors.name = `Use at most ${MAX_ENDPOINT_NAME} characters.`;
  else if (existing.some((endpoint) => endpoint.id !== editingId && endpoint.name.trim().toLowerCase() === name.toLowerCase())) {
    errors.name = "Another endpoint already uses this name.";
  }
  const urlError = validateBaseUrl(draft.baseUrl);
  if (urlError) errors.baseUrl = urlError;
  const models = parseModelList(draft.models);
  const invalid = models.filter((model) => !isModelId(model));
  if (invalid.length > 0) {
    const shown = invalid.slice(0, 3).join(", ") + (invalid.length > 3 ? ", ..." : "");
    errors.models = `Not a valid model ID: ${shown}. Use 1 to 128 letters, digits, '.', '_', ':', '/' or '-'.`;
  } else if (models.length > MAX_ENDPOINT_MODELS) {
    errors.models = `Use at most ${MAX_ENDPOINT_MODELS} model IDs.`;
  }
  if (Object.keys(errors).length > 0) return { endpoint: null, errors };
  const id = editingId ?? endpointIdFromName(name, existing.map((endpoint) => endpoint.id));
  return { endpoint: { id, name, baseUrl: draft.baseUrl.trim(), models }, errors: null };
}

/** Per-model color key, `provider/model` (Rust `ModelRef::color_key`). */
export function colorKey(ref: Pick<ModelRef, "provider" | "model">) {
  return `${ref.provider}/${ref.model}`;
}

/** Identity of a model reference including its effort and endpoint, for duplicate checks. */
export function sameModelRef(a: ModelRef, b: ModelRef) {
  return a.provider === b.provider && a.model === b.model
    && (a.effort ?? null) === (b.effort ?? null) && (a.endpoint ?? null) === (b.endpoint ?? null);
}

/** Normalized reference: effort only when set and not OpenAI-compatible; endpoint only for OpenAI-compatible. */
export function modelRef(provider: TextProviderKind, model: string, effort?: Effort | null, endpoint?: string | null): ModelRef {
  const ref: ModelRef = { provider, model };
  if (effort && provider !== "openai_compatible") ref.effort = effort;
  if (endpoint && provider === "openai_compatible") ref.endpoint = endpoint;
  return ref;
}

/** The effort to keep when switching models: the current one if offered, else the model default. */
export function effortForModel(model: TextModelInfo, current: ModelRef["effort"]): Effort | null {
  if (current && model.efforts.includes(current)) return current;
  return model.defaultEffort ?? model.efforts[0] ?? null;
}

export function findCatalogModel(models: readonly TextModelInfo[], ref: Pick<ModelRef, "provider" | "model">) {
  return models.find((model) => model.provider === ref.provider && model.id === ref.model) ?? null;
}

/** Display label: catalog label, else the model id. */
export function modelRefLabel(ref: Pick<ModelRef, "provider" | "model">, models: readonly TextModelInfo[]) {
  return findCatalogModel(models, ref)?.label ?? ref.model;
}

/** Catalog color for a model, else the provider default (Rust `runtime::model_color` without overrides). */
export function defaultModelColor(ref: Pick<ModelRef, "provider" | "model">, models: readonly TextModelInfo[]) {
  return findCatalogModel(models, ref)?.color ?? defaultProviderColor(ref.provider);
}

/** Color shown for a model: the saved override, else the catalog or provider default. */
export function modelColorFor(ref: Pick<ModelRef, "provider" | "model">, overrides: Record<string, string>, models: readonly TextModelInfo[]) {
  return overrides[colorKey(ref)] ?? defaultModelColor(ref, models);
}

/**
 * Whether the provider for `ref` lacks a saved key. Unknown status (null keys) never warns.
 * OpenAI-compatible keys are optional (local servers need none), so they never warn either.
 */
export function needsKey(ref: Pick<ModelRef, "provider">, keys: TextProviderKeys | null) {
  if (!keys) return false;
  switch (ref.provider) {
    case "openai": return !keys.openai;
    case "anthropic": return !keys.anthropic;
    case "gemini": return !keys.gemini;
    default: return false;
  }
}

export type FavoriteResult = { favorites: ModelRef[]; error: null } | { favorites: null; error: string };

export function addFavorite(favorites: readonly ModelRef[], ref: ModelRef): FavoriteResult {
  if (favorites.length >= MAX_FAVORITES) return { favorites: null, error: `You can keep at most ${MAX_FAVORITES} favorite models.` };
  if (favorites.some((item) => sameModelRef(item, ref))) return { favorites: null, error: "This model and effort is already a favorite." };
  return { favorites: [...favorites, ref], error: null };
}

/** Moves the favorite at `index` by `delta` positions; out-of-range moves return the list unchanged. */
export function moveFavorite(favorites: readonly ModelRef[], index: number, delta: number) {
  const target = index + delta;
  if (index < 0 || index >= favorites.length || target < 0 || target >= favorites.length) return [...favorites];
  const next = [...favorites];
  const [item] = next.splice(index, 1);
  next.splice(target, 0, item);
  return next;
}

export function removeFavorite(favorites: readonly ModelRef[], index: number) {
  return favorites.filter((_, position) => position !== index);
}

/** Sets the effort of one favorite; rejects efforts the model does not offer and duplicates. */
export function setFavoriteEffort(favorites: readonly ModelRef[], index: number, effort: Effort, model: TextModelInfo | null): FavoriteResult {
  const result = setRefEffort(favorites, index, effort, model, "That favorite no longer exists.", "This model and effort is already a favorite.");
  return result.error === null ? { favorites: result.refs, error: null } : { favorites: null, error: result.error };
}

function setRefEffort(refs: readonly ModelRef[], index: number, effort: Effort, model: TextModelInfo | null, missing: string, duplicate: string):
  { refs: ModelRef[]; error: null } | { refs: null; error: string } {
  const current = refs[index];
  if (!current) return { refs: null, error: missing };
  if (!model || !model.efforts.includes(effort)) return { refs: null, error: "This model does not offer that effort." };
  const updated: ModelRef = { ...current, effort };
  if (refs.some((item, position) => position !== index && sameModelRef(item, updated))) return { refs: null, error: duplicate };
  return { refs: refs.map((item, position) => (position === index ? updated : item)), error: null };
}

// ---------------------------------------------------------------------------
// Ensemble profiles (Models -> Text models). Limits mirror the Rust rules.
// ---------------------------------------------------------------------------

export const MAX_ENSEMBLE_PROFILES = 20;
export const MAX_PROFILE_MEMBERS = 9;
export const MAX_PROFILE_NAME = 40;
export const ENSEMBLE_MAX_USD = { min: 0, max: 5 } as const;

export type MembersResult = { members: ModelRef[]; error: null } | { members: null; error: string };

/** Adds a member; the same model may appear with different efforts, but not twice with the same one. */
export function addProfileMember(members: readonly ModelRef[], ref: ModelRef): MembersResult {
  if (members.length >= MAX_PROFILE_MEMBERS) return { members: null, error: `A profile has at most ${MAX_PROFILE_MEMBERS} models.` };
  if (members.some((item) => sameModelRef(item, ref))) return { members: null, error: "This model and effort is already in the profile." };
  return { members: [...members, ref], error: null };
}

export function setMemberEffort(members: readonly ModelRef[], index: number, effort: Effort, model: TextModelInfo | null): MembersResult {
  const result = setRefEffort(members, index, effort, model, "That model is no longer in the profile.", "This model and effort is already in the profile.");
  return result.error === null ? { members: result.refs, error: null } : { members: null, error: result.error };
}

export type ProfileDraft = { name: string; members: ModelRef[] };
export type ProfileErrors = Partial<Record<keyof ProfileDraft, string>>;

/**
 * Validates a profile draft: at most 20 profiles, names of 1 to 40 characters (unique, ignoring
 * case), 1 to 9 members without exact duplicates, and an id `[a-z0-9_-]{1,40}` derived from the
 * name when adding (fixed when editing). Returns the profile to save, or the field errors.
 */
export function validateProfileDraft(draft: ProfileDraft, existing: readonly EnsembleProfile[], editingId: string | null):
  { profile: EnsembleProfile; errors: null } | { profile: null; errors: ProfileErrors } {
  const errors: ProfileErrors = {};
  const name = draft.name.trim();
  if (editingId === null && existing.length >= MAX_ENSEMBLE_PROFILES) errors.name = `Vellora supports at most ${MAX_ENSEMBLE_PROFILES} ensemble profiles.`;
  else if (!name) errors.name = "Enter a name.";
  else if ([...name].length > MAX_PROFILE_NAME) errors.name = `Use at most ${MAX_PROFILE_NAME} characters.`;
  else if (existing.some((profile) => profile.id !== editingId && profile.name.trim().toLowerCase() === name.toLowerCase())) {
    errors.name = "Another profile already uses this name.";
  }
  if (draft.members.length === 0) errors.members = "Add at least one model.";
  else if (draft.members.length > MAX_PROFILE_MEMBERS) errors.members = `Use at most ${MAX_PROFILE_MEMBERS} models.`;
  else if (draft.members.some((ref, index) => draft.members.findIndex((item) => sameModelRef(item, ref)) !== index)) {
    errors.members = "Each model and effort can appear only once.";
  }
  if (Object.keys(errors).length > 0) return { profile: null, errors };
  const id = editingId ?? profileIdFromName(name, existing.map((profile) => profile.id));
  if (!isEndpointId(id) || existing.some((profile) => profile.id === id && profile.id !== editingId)) {
    return { profile: null, errors: { name: "This name does not give a usable profile ID. Try another name." } };
  }
  return { profile: { id, name, members: [...draft.members] }, errors: null };
}

/** Parses the per-run ensemble limit in USD; returns an error message when it is out of range. */
export function parseEnsembleLimit(text: string): { value: number; error: null } | { value: null; error: string } {
  const trimmed = text.trim().replace(/^\$/, "");
  const value = Number(trimmed);
  if (!trimmed || !Number.isFinite(value) || value < ENSEMBLE_MAX_USD.min || value > ENSEMBLE_MAX_USD.max) {
    return { value: null, error: `Enter an amount between $${ENSEMBLE_MAX_USD.min} and $${ENSEMBLE_MAX_USD.max}.` };
  }
  return { value: Math.round(value * 10_000) / 10_000, error: null };
}

/** Color keys still used by the quick model, favorites, presets or ensemble profiles. */
export function usedColorKeys(assist: Pick<TextAssistSettings, "quickModel" | "favoriteModels" | "presets" | "ensembleProfiles">) {
  const keys = new Set<string>([colorKey(assist.quickModel), ...assist.favoriteModels.map(colorKey)]);
  for (const preset of assist.presets) if (preset.model) keys.add(colorKey(preset.model));
  for (const profile of assist.ensembleProfiles) for (const member of profile.members) keys.add(colorKey(member));
  return keys;
}

/** Drops color overrides that no saved model uses any more. */
export function pruneModelColors(assist: TextAssistSettings): Record<string, string> {
  const used = usedColorKeys(assist);
  return Object.fromEntries(Object.entries(assist.modelColors).filter(([key]) => used.has(key)));
}

/** Option value for a model picker; unambiguous even for ids containing ':' or '/'. */
export function modelOptionValue(ref: Pick<ModelRef, "provider" | "model" | "endpoint">) {
  return JSON.stringify([ref.provider, ref.endpoint ?? null, ref.model]);
}

export function parseModelOptionValue(value: string): { provider: TextProviderKind; endpoint: string | null; model: string } | null {
  try {
    const parsed: unknown = JSON.parse(value);
    if (Array.isArray(parsed) && parsed.length === 3 && typeof parsed[0] === "string" && typeof parsed[2] === "string"
      && (parsed[1] === null || typeof parsed[1] === "string")) {
      return { provider: parsed[0] as TextProviderKind, endpoint: parsed[1] as string | null, model: parsed[2] };
    }
  } catch { /* not a model option value */ }
  return null;
}

/** Catalog models grouped by provider (display order), quick candidates first within each group. */
export function groupCatalogModels(models: readonly TextModelInfo[]) {
  return CATALOG_PROVIDERS.map((provider) => {
    const own = models.filter((model) => model.provider === provider);
    return { provider, models: [...own.filter((model) => model.quickCandidate), ...own.filter((model) => !model.quickCandidate)] };
  }).filter((group) => group.models.length > 0);
}

/**
 * Saved-key status for all providers; `endpointIds` are the OpenAI-compatible endpoints to check.
 * Older responses never overwrite newer ones.
 */
export function useTextProviderKeys(endpointIds: readonly string[]) {
  const [keys, setKeys] = useState<TextProviderKeys | null>(null);
  const [error, setError] = useState<string | null>(null);
  const requestId = useRef(0);
  const idsKey = endpointIds.join("\n");
  // `reload` may run from a closure created before the endpoint list changed (for example right
  // after adding an endpoint), so it reads the ids of the latest render instead of capturing them.
  const latestIds = useRef(idsKey);
  latestIds.current = idsKey;

  const load = useCallback(async () => {
    const id = ++requestId.current;
    const ids = latestIds.current;
    try {
      const value = await getTextProviderKeys(ids ? ids.split("\n") : []);
      if (id === requestId.current) { setKeys(value); setError(null); }
    } catch (reason) {
      if (id === requestId.current) setError(String(reason));
    }
  }, []);

  useEffect(() => {
    void load();
    // Invalidate in-flight reads so they cannot update an unmounted view.
    return () => { requestId.current += 1; };
  }, [load, idsKey]);

  return { keys, error, reload: load };
}

/**
 * Settings patch after the endpoint list changed: favorites and ensemble members that point at a
 * removed endpoint, or at a model the endpoint no longer lists, are dropped (profiles left without
 * members are removed), and color overrides nobody uses are pruned.
 */
export function endpointsPatch(assist: TextAssistSettings, endpoints: CompatEndpoint[]):
  Pick<TextAssistSettings, "openaiCompatible" | "favoriteModels" | "ensembleProfiles" | "modelColors"> {
  const byId = new Map(endpoints.map((endpoint) => [endpoint.id, endpoint]));
  const available = (ref: ModelRef) => {
    if (ref.provider !== "openai_compatible") return true;
    const endpoint = ref.endpoint ? byId.get(ref.endpoint) : undefined;
    return Boolean(endpoint && endpoint.models.includes(ref.model));
  };
  const favoriteModels = assist.favoriteModels.filter(available);
  const ensembleProfiles = assist.ensembleProfiles
    .map((profile) => (profile.members.every(available) ? profile : { ...profile, members: profile.members.filter(available) }))
    .filter((profile) => profile.members.length > 0);
  const next = { ...assist, openaiCompatible: endpoints, favoriteModels, ensembleProfiles };
  return { openaiCompatible: endpoints, favoriteModels, ensembleProfiles, modelColors: pruneModelColors(next) };
}

/** Picker label: "GPT-6 Luna - $0.10 in / $0.50 out per 1M tokens", plus retirement and key hints. */
export function catalogOptionLabel(model: TextModelInfo, missingKey = false) {
  const retiring = model.shutdown ? ` (retires ${model.shutdown})` : "";
  return `${model.label} - ${formatModelPrice(model)}${retiring}${missingKey ? " - needs key" : ""}`;
}

/** Readable name for a favorite: "GPT-6 Luna, None" or "llama3.2 on Ollama". */
export function favoriteName(ref: ModelRef, models: readonly TextModelInfo[], endpoints: readonly CompatEndpoint[]) {
  if (ref.provider === "openai_compatible") {
    const endpoint = endpoints.find((item) => item.id === ref.endpoint);
    return `${ref.model} on ${endpoint?.name ?? ref.endpoint ?? "an unknown endpoint"}`;
  }
  const label = modelRefLabel(ref, models);
  return ref.effort ? `${label}, ${effortLabel(ref.effort)}` : label;
}

export type ColorResult = { modelColors: Record<string, string>; error: null } | { modelColors: null; error: string };

/**
 * Saves a color override for `key`. Choosing the default color removes the override. Unused
 * overrides are pruned first so the 64-entry limit only counts colors that are still in use.
 */
export function setModelColor(assist: TextAssistSettings, key: string, color: string, defaultColor: string): ColorResult {
  if (!isHexColor(color)) return { modelColors: null, error: "Enter a color as #rrggbb, for example #10a37f." };
  const value = color.toLowerCase();
  const colors = { ...pruneModelColors(assist) };
  if (value === defaultColor.toLowerCase()) delete colors[key];
  else colors[key] = value;
  if (Object.keys(colors).length > MAX_MODEL_COLORS) {
    return { modelColors: null, error: `Vellora keeps at most ${MAX_MODEL_COLORS} model colors.` };
  }
  return { modelColors: colors, error: null };
}

export function resetModelColor(colors: Record<string, string>, key: string) {
  return Object.fromEntries(Object.entries(colors).filter(([item]) => item !== key));
}
