import { useEffect, useId, useMemo, useRef, useState } from "react";
import { ArrowDown, ArrowUp, KeyRound, Pencil, Plus, Trash2 } from "lucide-react";
import { deleteTextProviderKey, saveTextProviderKey } from "../lib/api";
import { EnsembleProfilesSection, ModelOptions } from "./EnsembleProfilesSection";
import {
  MAX_ENDPOINTS,
  MAX_FAVORITES,
  addFavorite,
  colorKey,
  defaultModelColor,
  effortForModel,
  effortLabel,
  endpointIdFromName,
  endpointsPatch,
  favoriteName,
  findCatalogModel,
  isHexColor,
  isLocalBaseUrl,
  modelColorFor,
  modelOptionValue,
  modelRef,
  modelRefLabel,
  moveFavorite,
  needsKey,
  parseModelOptionValue,
  providerLabel,
  pruneModelColors,
  removeFavorite,
  resetModelColor,
  setFavoriteEffort,
  setModelColor,
  useTextAssistOverview,
  useTextProviderKeys,
  validateEndpointDraft,
  type EndpointDraft,
  type EndpointErrors,
} from "../lib/textAssist";
import type {
  AppSettings,
  CompatEndpoint,
  Effort,
  KeyedTextProvider,
  ModelRef,
  SettingsChange,
  TextAssistSettings,
  TextModelInfo,
  TextProviderKeys,
} from "../lib/types";

type Props = {
  settings: AppSettings;
  onSettingsChange: SettingsChange;
};

type Section = "endpoints" | "favorites" | "profiles" | "colors";
type Feedback = { text: string; error: boolean };
type SectionFeedback = Feedback & { section: Section };
type Save = (section: Section, patch: Partial<TextAssistSettings>, success: string) => Promise<boolean>;

const providerPrivacy: Record<"openai" | "anthropic" | "gemini", string> = {
  openai: "Text Assist sends OpenAI requests with store:false, so they are not kept for later retrieval in your OpenAI account.",
  anthropic: "Requests use your Anthropic API key under Anthropic's API terms. Only the selected text and the instruction are sent.",
  gemini: "Free tier: Google may use the text you send to improve its products. Paid tier (billing enabled for the key's project): it does not.",
};

/** Models -> Text models: provider keys, OpenAI-compatible endpoints, favorites and colors. */
export function TextModelsView({ settings, onSettingsChange }: Props) {
  const assist = settings.textAssist;
  const { overview, error: modelsError } = useTextAssistOverview();
  const endpointIds = useMemo(() => assist.openaiCompatible.map((endpoint) => endpoint.id), [assist.openaiCompatible]);
  const { keys, error: keysError, reload: reloadKeys } = useTextProviderKeys(endpointIds);
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<SectionFeedback | null>(null);
  const catalog = overview?.models ?? [];
  const busyRef = useRef(false);

  const save: Save = async (section, patch, success) => {
    if (busyRef.current) return false;
    busyRef.current = true;
    setBusy(true);
    setFeedback(null);
    try {
      await onSettingsChange({ textAssist: { ...assist, ...patch } });
      setFeedback({ section, text: success, error: false });
      return true;
    } catch (error) {
      setFeedback({ section, text: `Could not save text model settings: ${String(error)}`, error: true });
      return false;
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const report = (section: Section, text: string, error: boolean) => setFeedback({ section, text, error });
  const feedbackFor = (section: Section) => (feedback?.section === section ? feedback : null);

  return (
    <div className="settings-stack ta-models">
      {modelsError && <p className="error-text" role="alert">Could not load the model list: {modelsError}</p>}
      {keysError && <p className="error-text" role="alert">Could not read which keys are saved: {keysError}</p>}
      <ProviderKeys keys={keys} onChanged={() => void reloadKeys()} />
      <Endpoints assist={assist} keys={keys} busy={busy} save={save} report={report}
        feedback={feedbackFor("endpoints")} onKeysChanged={() => void reloadKeys()} />
      <Favorites assist={assist} catalog={catalog} loading={!overview && !modelsError} keys={keys} busy={busy}
        save={save} report={report} feedback={feedbackFor("favorites")} />
      <EnsembleProfilesSection assist={assist} catalog={catalog} loading={!overview && !modelsError} keys={keys} busy={busy}
        save={(patch, success) => save("profiles", patch, success)} feedback={feedbackFor("profiles")} />
      <ModelColors assist={assist} catalog={catalog} busy={busy} save={save} report={report} feedback={feedbackFor("colors")} />
    </div>
  );
}

function FeedbackText({ feedback }: { feedback: Feedback | null }) {
  if (!feedback) return null;
  return <p className={feedback.error ? "error-text ta-feedback" : "success-text ta-feedback"} role={feedback.error ? "alert" : "status"}>{feedback.text}</p>;
}

function keyStatusText(saved: boolean | null | undefined) {
  if (saved === null || saved === undefined) return "Checking...";
  return saved ? "Key saved" : "No key saved";
}

// ---------------------------------------------------------------------------
// Provider keys
// ---------------------------------------------------------------------------

function ProviderKeys({ keys, onChanged }: { keys: TextProviderKeys | null; onChanged: () => void }) {
  const ids = useId();
  return (
    <section className="panel" aria-labelledby={`${ids}-title`}>
      <h3 id={`${ids}-title`}>Provider keys</h3>
      <p className="hint">Keys are stored in Windows Credential Manager and are never shown again. Only the selected text is sent, and only to the provider of the model you use.</p>
      <div className="ta-provider-list">
        <article className="ta-provider" aria-labelledby={`${ids}-openai`}>
          <div className="ta-provider-head">
            <h4 id={`${ids}-openai`}>OpenAI</h4>
            <span className={`ta-key-status ${keys?.openai ? "saved" : ""}`}>{keyStatusText(keys?.openai)}</span>
          </div>
          <p className="hint">Shared with dictation. Add, replace or remove it under Settings, General, OpenAI key.</p>
          <p className="hint ta-privacy">{providerPrivacy.openai}</p>
        </article>
        {(["anthropic", "gemini"] as const).map((provider) => (
          <article className="ta-provider" key={provider} aria-labelledby={`${ids}-${provider}`}>
            <div className="ta-provider-head">
              <h4 id={`${ids}-${provider}`}>{providerLabel(provider)}</h4>
              <span className={`ta-key-status ${keys?.[provider] ? "saved" : ""}`}>{keyStatusText(keys?.[provider])}</span>
            </div>
            <KeyEditor provider={provider} endpointId={null} name={providerLabel(provider)} saved={keys ? keys[provider] : null} onChanged={onChanged} />
            <p className="hint ta-privacy">{providerPrivacy[provider]}</p>
          </article>
        ))}
      </div>
    </section>
  );
}

type KeyEditorProps = {
  provider: KeyedTextProvider;
  endpointId: string | null;
  /** Provider or endpoint name used in labels, e.g. "Anthropic" or "Ollama". */
  name: string;
  saved: boolean | null;
  onChanged: () => void;
};

function KeyEditor({ provider, endpointId, name, saved, onChanged }: KeyEditorProps) {
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const removeButton = useRef<HTMLButtonElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);

  useEffect(() => { if (confirm) cancelButton.current?.focus(); }, [confirm]);

  async function saveKey() {
    const key = draft.trim();
    if (!key || busy) return;
    setBusy(true);
    setFeedback(null);
    try {
      await saveTextProviderKey(provider, endpointId, key);
      setDraft("");
      setFeedback({ text: `${name} key saved.`, error: false });
      onChanged();
    } catch (error) {
      setFeedback({ text: `Could not save the ${name} key: ${String(error)}`, error: true });
    } finally {
      setBusy(false);
    }
  }

  async function removeKey() {
    if (busy) return;
    setBusy(true);
    setFeedback(null);
    try {
      await deleteTextProviderKey(provider, endpointId);
      setConfirm(false);
      setFeedback({ text: `${name} key removed.`, error: false });
      onChanged();
    } catch (error) {
      setFeedback({ text: `Could not remove the ${name} key: ${String(error)}`, error: true });
    } finally {
      setBusy(false);
    }
  }

  function cancel() {
    setConfirm(false);
    window.requestAnimationFrame(() => removeButton.current?.focus());
  }

  return (
    <div className="ta-key-editor">
      <form className="input-row" onSubmit={(event) => { event.preventDefault(); void saveKey(); }}>
        <KeyRound size={17} aria-hidden="true" />
        <input type="password" value={draft} disabled={busy} autoComplete="off" spellCheck={false}
          aria-label={saved ? `Replace ${name} API key` : `${name} API key`}
          placeholder={saved ? `Replace saved ${name} key` : `Paste ${name} API key`}
          onChange={(event) => setDraft(event.target.value)} />
        <button type="submit" disabled={busy || !draft.trim()} aria-label={`Save ${name} key`}>{busy ? "Saving..." : "Save"}</button>
        {saved && (
          <button ref={removeButton} type="button" className="danger" disabled={busy || confirm}
            aria-label={`Remove ${name} key`} title={`Remove saved ${name} key`} onClick={() => setConfirm(true)}>
            <Trash2 size={15} aria-hidden="true" />
          </button>
        )}
      </form>
      {confirm && (
        <div className="inline-confirmation" role="group" aria-label={`Confirm removing the ${name} key`}>
          <span>Remove the saved {name} key from Windows Credential Manager? Models that use it stop working until you add a key again.</span>
          <button type="button" className="icon-button danger-text" disabled={busy} onClick={() => void removeKey()}>Remove key</button>
          <button ref={cancelButton} type="button" className="icon-button" disabled={busy} onClick={cancel}>Cancel</button>
        </div>
      )}
      <FeedbackText feedback={feedback} />
    </div>
  );
}

// ---------------------------------------------------------------------------
// OpenAI-compatible endpoints
// ---------------------------------------------------------------------------

type SectionProps = {
  assist: TextAssistSettings;
  busy: boolean;
  save: Save;
  report: (section: Section, text: string, error: boolean) => void;
  feedback: Feedback | null;
};

function Endpoints({ assist, keys, busy, save, report, feedback, onKeysChanged }: SectionProps & {
  keys: TextProviderKeys | null;
  onKeysChanged: () => void;
}) {
  const ids = useId();
  const endpoints = assist.openaiCompatible;
  const [editing, setEditing] = useState<string | "new" | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);
  const addButton = useRef<HTMLButtonElement>(null);
  const full = endpoints.length >= MAX_ENDPOINTS;

  function closeForm() {
    setEditing(null);
    window.requestAnimationFrame(() => addButton.current?.focus());
  }

  async function submit(endpoint: CompatEndpoint, key: string) {
    const editingId = editing === "new" ? null : editing;
    const next = editingId ? endpoints.map((item) => (item.id === editingId ? endpoint : item)) : [...endpoints, endpoint];
    const patch = endpointsPatch(assist, next);
    const dropped = assist.favoriteModels.length - patch.favoriteModels.length;
    const droppedProfiles = assist.ensembleProfiles.length - patch.ensembleProfiles.length;
    const note = (dropped > 0 ? ` ${dropped} favorite${dropped === 1 ? "" : "s"} for removed models ${dropped === 1 ? "was" : "were"} removed.` : "")
      + (droppedProfiles > 0 ? ` ${droppedProfiles} ensemble profile${droppedProfiles === 1 ? "" : "s"} without models ${droppedProfiles === 1 ? "was" : "were"} removed.` : "");
    const ok = await save("endpoints", patch, `${endpoint.name} ${editingId ? "updated" : "added"}.${note}`);
    if (!ok) return false;
    if (key) {
      try {
        await saveTextProviderKey("openai_compatible", endpoint.id, key);
      } catch (error) {
        report("endpoints", `${endpoint.name} was saved, but its key could not be saved: ${String(error)}`, true);
      }
      onKeysChanged();
    }
    closeForm();
    return true;
  }

  async function remove(endpoint: CompatEndpoint) {
    const patch = endpointsPatch(assist, endpoints.filter((item) => item.id !== endpoint.id));
    const ok = await save("endpoints", patch, `${endpoint.name} removed.`);
    if (!ok) return;
    setConfirmRemove(null);
    if (editing === endpoint.id) setEditing(null);
    window.requestAnimationFrame(() => addButton.current?.focus());
    if (keys?.compat[endpoint.id] === false) return;
    try {
      await deleteTextProviderKey("openai_compatible", endpoint.id);
    } catch (error) {
      report("endpoints", `${endpoint.name} was removed, but its saved key could not be deleted: ${String(error)}`, true);
    }
    onKeysChanged();
  }

  return (
    <section className="panel" aria-labelledby={`${ids}-title`}>
      <h3 id={`${ids}-title`}>OpenAI-compatible endpoints</h3>
      <p className="hint">
        Use any server that speaks the OpenAI Chat Completions format, such as Groq, Cerebras or OpenRouter.
        Local servers such as Ollama or LM Studio on 127.0.0.1 or localhost are the only fully offline option.
        Vellora sends no reasoning effort to these endpoints.
      </p>
      <FeedbackText feedback={feedback} />
      {endpoints.length > 0 && (
        <ul className="ta-endpoint-list" aria-label="OpenAI-compatible endpoints">
          {endpoints.map((endpoint) => {
            const local = isLocalBaseUrl(endpoint.baseUrl);
            const saved = keys ? (keys.compat[endpoint.id] ?? false) : null;
            return (
              <li key={endpoint.id} className="ta-endpoint">
                <div className="ta-provider-head">
                  <h4>{endpoint.name}</h4>
                  <span className="chip">{local ? "On this computer" : "Remote"}</span>
                  <span className={`ta-key-status ${saved ? "saved" : ""}`}>{saved === null ? "Checking..." : saved ? "Key saved" : local ? "No key (optional)" : "No key saved"}</span>
                </div>
                <p className="hint ta-endpoint-meta"><span className="ta-mono">{endpoint.baseUrl}</span> · ID {endpoint.id}</p>
                <p className="hint ta-endpoint-meta">
                  {endpoint.models.length === 0 ? "No model IDs yet. Edit the endpoint to add some." : `Models: ${endpoint.models.join(", ")}`}
                </p>
                <div className="history-toolbar ta-row-actions">
                  <button type="button" className="icon-button" disabled={busy} aria-label={`Edit ${endpoint.name}`}
                    onClick={() => { setConfirmRemove(null); setEditing(endpoint.id); }}>
                    <Pencil size={15} aria-hidden="true" /> Edit
                  </button>
                  <button type="button" className="icon-button danger-text" disabled={busy} aria-label={`Remove ${endpoint.name}`}
                    onClick={() => setConfirmRemove(endpoint.id)}>
                    <Trash2 size={15} aria-hidden="true" /> Remove
                  </button>
                </div>
                {confirmRemove === endpoint.id && (
                  <div className="inline-confirmation" role="group" aria-label={`Confirm removing ${endpoint.name}`}>
                    <span>Remove {endpoint.name}? Its saved key and its favorite models are removed too.</span>
                    <button type="button" className="icon-button danger-text" disabled={busy} onClick={() => void remove(endpoint)}>Remove endpoint</button>
                    <button type="button" className="icon-button" disabled={busy} onClick={() => setConfirmRemove(null)}>Cancel</button>
                  </div>
                )}
                {editing === endpoint.id ? (
                  <EndpointForm initial={endpoint} existing={endpoints} busy={busy} onSubmit={submit} onCancel={closeForm} />
                ) : (
                  <KeyEditor provider="openai_compatible" endpointId={endpoint.id} name={endpoint.name} saved={saved} onChanged={onKeysChanged} />
                )}
              </li>
            );
          })}
        </ul>
      )}
      {editing === "new" ? (
        <EndpointForm initial={null} existing={endpoints} busy={busy} onSubmit={submit} onCancel={closeForm} />
      ) : (
        <div className="history-toolbar ta-row-actions">
          <button ref={addButton} type="button" className="icon-button" disabled={busy || full}
            aria-describedby={full ? `${ids}-full` : undefined}
            onClick={() => { setConfirmRemove(null); setEditing("new"); }}>
            <Plus size={15} aria-hidden="true" /> Add endpoint
          </button>
        </div>
      )}
      {full && <p className="hint" id={`${ids}-full`}>Vellora supports at most {MAX_ENDPOINTS} endpoints. Remove one to add another.</p>}
    </section>
  );
}

function EndpointForm({ initial, existing, busy, onSubmit, onCancel }: {
  initial: CompatEndpoint | null;
  existing: readonly CompatEndpoint[];
  busy: boolean;
  onSubmit: (endpoint: CompatEndpoint, key: string) => Promise<boolean>;
  onCancel: () => void;
}) {
  const ids = useId();
  const [draft, setDraft] = useState<EndpointDraft>(() => ({
    name: initial?.name ?? "",
    baseUrl: initial?.baseUrl ?? "",
    models: initial?.models.join("\n") ?? "",
  }));
  const [key, setKey] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const nameInput = useRef<HTMLInputElement>(null);
  const editingId = initial?.id ?? null;

  useEffect(() => { nameInput.current?.focus(); }, []);

  const result = validateEndpointDraft(draft, existing, editingId);
  const errors: EndpointErrors = submitted && result.errors ? result.errors : {};
  const previewId = editingId ?? endpointIdFromName(draft.name, existing.map((endpoint) => endpoint.id));
  const title = initial ? `Edit ${initial.name}` : "Add an endpoint";

  async function submit() {
    setSubmitted(true);
    if (!result.endpoint || busy) {
      const firstInvalid = (["name", "baseUrl", "models"] as const).find((field) => result.errors?.[field]);
      if (firstInvalid) document.getElementById(`${ids}-${firstInvalid}`)?.focus();
      return;
    }
    await onSubmit(result.endpoint, key.trim());
  }

  const field = (name: keyof EndpointDraft) => ({
    id: `${ids}-${name}`,
    value: draft[name],
    disabled: busy,
    "aria-invalid": errors[name] ? true : undefined,
    "aria-describedby": errors[name] ? `${ids}-${name}-error` : `${ids}-${name}-hint`,
    onChange: (event: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => setDraft({ ...draft, [name]: event.target.value }),
  });

  return (
    <form className="ta-endpoint-form" noValidate aria-label={title} onSubmit={(event) => { event.preventDefault(); void submit(); }}>
      <h4>{title}</h4>
      <label className="field">
        <span>Name</span>
        <input ref={nameInput} type="text" maxLength={80} autoComplete="off" {...field("name")} />
      </label>
      {errors.name ? <p className="error-text ta-field-error" id={`${ids}-name-error`}>{errors.name}</p>
        : <p className="hint ta-field-hint" id={`${ids}-name-hint`}>{editingId ? `ID ${editingId} (fixed).` : `ID ${previewId}, derived from the name.`}</p>}
      <label className="field">
        <span>Base URL</span>
        <input type="url" autoComplete="off" spellCheck={false} placeholder="http://127.0.0.1:11434/v1" {...field("baseUrl")} />
      </label>
      {errors.baseUrl ? <p className="error-text ta-field-error" id={`${ids}-baseUrl-error`}>{errors.baseUrl}</p>
        : <p className="hint ta-field-hint" id={`${ids}-baseUrl-hint`}>HTTPS, or plain HTTP only for 127.0.0.1, localhost or [::1].</p>}
      <label className="field">
        <span>Model IDs</span>
        <textarea rows={3} spellCheck={false} placeholder={"llama3.2\nqwen3:8b"} {...field("models")} />
      </label>
      {errors.models ? <p className="error-text ta-field-error" id={`${ids}-models-error`}>{errors.models}</p>
        : <p className="hint ta-field-hint" id={`${ids}-models-hint`}>One per line or separated by commas, exactly as the server names them. Up to 50.</p>}
      {!initial && (
        <>
          <label className="field">
            <span>API key (optional)</span>
            <input type="password" value={key} disabled={busy} autoComplete="off" spellCheck={false}
              aria-describedby={`${ids}-key-hint`} onChange={(event) => setKey(event.target.value)} />
          </label>
          <p className="hint ta-field-hint" id={`${ids}-key-hint`}>Local servers usually need none. Stored in Windows Credential Manager.</p>
        </>
      )}
      <div className="history-toolbar ta-row-actions">
        <button type="submit" className="icon-button" disabled={busy}>{initial ? "Save endpoint" : "Add endpoint"}</button>
        <button type="button" className="icon-button" disabled={busy} onClick={onCancel}>Cancel</button>
      </div>
    </form>
  );
}

// ---------------------------------------------------------------------------
// Favorites
// ---------------------------------------------------------------------------

function Favorites({ assist, catalog, loading, keys, busy, save, report, feedback }: SectionProps & {
  catalog: readonly TextModelInfo[];
  loading: boolean;
  keys: TextProviderKeys | null;
}) {
  const ids = useId();
  const favorites = assist.favoriteModels;
  const endpoints = assist.openaiCompatible;
  const [choice, setChoice] = useState("");
  const [effort, setEffort] = useState<Effort | "">("");
  const pendingFocus = useRef<{ index: number; action: "up" | "down" } | null>(null);
  const full = favorites.length >= MAX_FAVORITES;

  const chosen = parseModelOptionValue(choice);
  const chosenModel = chosen && chosen.provider !== "openai_compatible" ? findCatalogModel(catalog, chosen) : null;
  const chosenEfforts = chosenModel?.efforts ?? [];

  // Keep keyboard focus on the moved favorite once the saved order arrives.
  useEffect(() => {
    const pending = pendingFocus.current;
    if (!pending || busy) return;
    pendingFocus.current = null;
    const button = (action: string) => document.getElementById(`${ids}-fav-${pending.index}-${action}`) as HTMLButtonElement | null;
    const target = button(pending.action);
    (target && !target.disabled ? target : button(pending.action === "up" ? "down" : "up"))?.focus();
  }, [favorites, busy, ids]);

  function selectChoice(value: string) {
    setChoice(value);
    const parsed = parseModelOptionValue(value);
    const model = parsed && parsed.provider !== "openai_compatible" ? findCatalogModel(catalog, parsed) : null;
    setEffort(model ? (effortForModel(model, effort || null) ?? "") : "");
  }

  function add() {
    if (!chosen) return;
    if (chosen.provider !== "openai_compatible" && !chosenModel) return;
    const ref = modelRef(chosen.provider, chosen.model, effort || null, chosen.endpoint);
    const result = addFavorite(favorites, ref);
    if (result.error !== null) { report("favorites", result.error, true); return; }
    void save("favorites", { favoriteModels: result.favorites }, `${favoriteName(ref, catalog, endpoints)} added to favorites.`);
  }

  function move(index: number, delta: number) {
    const next = moveFavorite(favorites, index, delta);
    pendingFocus.current = { index: index + delta, action: delta < 0 ? "up" : "down" };
    void save("favorites", { favoriteModels: next }, `${favoriteName(favorites[index], catalog, endpoints)} moved to position ${index + delta + 1}.`);
  }

  function remove(index: number) {
    const ref = favorites[index];
    const favoriteModels = removeFavorite(favorites, index);
    const modelColors = pruneModelColors({ ...assist, favoriteModels });
    void save("favorites", { favoriteModels, modelColors }, `${favoriteName(ref, catalog, endpoints)} removed from favorites.`);
  }

  function changeEffort(index: number, value: Effort) {
    const ref = favorites[index];
    const result = setFavoriteEffort(favorites, index, value, findCatalogModel(catalog, ref));
    if (result.error !== null) { report("favorites", result.error, true); return; }
    void save("favorites", { favoriteModels: result.favorites }, `Effort for ${modelRefLabel(ref, catalog)} set to ${effortLabel(value)}.`);
  }

  const missingKey = (provider: ModelRef["provider"]) => needsKey({ provider }, keys);

  return (
    <section className="panel" aria-labelledby={`${ids}-title`}>
      <h3 id={`${ids}-title`}>Favorite models</h3>
      <p className="hint" id={`${ids}-hint`}>
        In the Rewrite panel, <kbd>Tab</kbd> and <kbd>Shift</kbd>+<kbd>Tab</kbd> cycle through these models in this order.
        Add a model several times to compare efforts. Up to {MAX_FAVORITES}.
      </p>
      <FeedbackText feedback={feedback} />
      {favorites.length === 0 ? (
        <p className="hint">No favorites yet. The Rewrite panel then uses only the Quick Polish model.</p>
      ) : (
        <ol className="ta-favorites" aria-label="Favorite models" aria-describedby={`${ids}-hint`}>
          {favorites.map((ref, index) => {
            const name = favoriteName(ref, catalog, endpoints);
            const model = ref.provider === "openai_compatible" ? null : findCatalogModel(catalog, ref);
            const endpoint = ref.provider === "openai_compatible" ? endpoints.find((item) => item.id === ref.endpoint) : null;
            const efforts = model?.efforts ?? [];
            const keyMissing = missingKey(ref.provider);
            return (
              <li key={`${index}-${modelOptionValue(ref)}-${ref.effort ?? ""}`} className="ta-favorite">
                <span className="ta-swatch" style={{ background: modelColorFor(ref, assist.modelColors, catalog) }} aria-hidden="true" />
                <span className="ta-favorite-name">
                  <strong>{index + 1}. {modelRefLabel(ref, catalog)}</strong>
                  <span className="ta-favorite-meta">
                    {ref.provider === "openai_compatible" ? `${endpoint?.name ?? ref.endpoint ?? "Unknown endpoint"} (OpenAI-compatible)` : providerLabel(ref.provider)}
                    {keyMissing && <span className="warning-text ta-needs-key"> · needs key</span>}
                  </span>
                </span>
                {efforts.length > 0 ? (
                  <select className="ta-effort" aria-label={`Effort for ${name}`} value={ref.effort ?? ""} disabled={busy}
                    onChange={(event) => changeEffort(index, event.target.value as Effort)}>
                    {!ref.effort && <option value="">Model default</option>}
                    {ref.effort && !efforts.includes(ref.effort) && <option value={ref.effort}>{effortLabel(ref.effort)} (not offered)</option>}
                    {efforts.map((item) => <option key={item} value={item}>{effortLabel(item)}</option>)}
                  </select>
                ) : (
                  <span className="ta-effort ta-effort-none">{ref.provider === "openai_compatible" ? "No effort sent" : ref.effort ? effortLabel(ref.effort) : "Default effort"}</span>
                )}
                <span className="ta-favorite-actions">
                  <button id={`${ids}-fav-${index}-up`} type="button" className="icon-button" disabled={busy || index === 0}
                    aria-label={`Move ${name} up`} title="Move up" onClick={() => move(index, -1)}>
                    <ArrowUp size={15} aria-hidden="true" />
                  </button>
                  <button id={`${ids}-fav-${index}-down`} type="button" className="icon-button" disabled={busy || index === favorites.length - 1}
                    aria-label={`Move ${name} down`} title="Move down" onClick={() => move(index, 1)}>
                    <ArrowDown size={15} aria-hidden="true" />
                  </button>
                  <button type="button" className="icon-button danger-text" disabled={busy}
                    aria-label={`Remove ${name} from favorites`} title="Remove" onClick={() => remove(index)}>
                    <Trash2 size={15} aria-hidden="true" />
                  </button>
                </span>
              </li>
            );
          })}
        </ol>
      )}

      <form className="ta-add-favorite" aria-label="Add a favorite model" onSubmit={(event) => { event.preventDefault(); add(); }}>
        <label className="field">
          <span>Model</span>
          <select value={choice} disabled={busy || full} onChange={(event) => selectChoice(event.target.value)}>
            <option value="">{loading ? "Loading models..." : "Choose a model"}</option>
            <ModelOptions catalog={catalog} endpoints={endpoints} keys={keys} />
          </select>
        </label>
        <label className="field">
          <span>Effort</span>
          <select value={effort} disabled={busy || full || chosenEfforts.length === 0} onChange={(event) => setEffort(event.target.value as Effort)}>
            {chosenEfforts.length === 0 && <option value="">{chosen?.provider === "openai_compatible" ? "No effort sent" : "Choose a model first"}</option>}
            {chosenEfforts.map((item) => <option key={item} value={item}>{effortLabel(item)}</option>)}
          </select>
        </label>
        <button type="submit" className="icon-button" disabled={busy || full || !chosen}>
          <Plus size={15} aria-hidden="true" /> Add favorite
        </button>
      </form>
      {full && <p className="hint">You have {MAX_FAVORITES} favorites, the maximum. Remove one to add another.</p>}
      {chosen && chosen.provider !== "openai_compatible" && missingKey(chosen.provider) && (
        <p className="warning-text ta-key-note">{providerLabel(chosen.provider)} has no saved key yet. Add it under Provider keys before you use this model.</p>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Model colors
// ---------------------------------------------------------------------------

type ColorEntry = { key: string; ref: ModelRef; label: string; usage: string };

function colorEntries(assist: TextAssistSettings, catalog: readonly TextModelInfo[]): ColorEntry[] {
  const entries = new Map<string, ColorEntry>();
  const addEntry = (ref: ModelRef, usage: string) => {
    const key = colorKey(ref);
    const existing = entries.get(key);
    if (existing) {
      if (!existing.usage.includes(usage)) existing.usage = `${existing.usage}, ${usage}`;
      return;
    }
    const provider = ref.provider === "openai_compatible"
      ? "OpenAI-compatible"
      : providerLabel(ref.provider);
    entries.set(key, { key, ref, label: modelRefLabel(ref, catalog), usage: `${provider} · ${usage}` });
  };
  addEntry(assist.quickModel, "Quick Polish");
  for (const ref of assist.favoriteModels) addEntry(ref, "favorite");
  for (const profile of assist.ensembleProfiles) for (const ref of profile.members) addEntry(ref, "ensemble");
  return [...entries.values()];
}

function ModelColors({ assist, catalog, busy, save, feedback }: SectionProps & { catalog: readonly TextModelInfo[] }) {
  const ids = useId();
  const entries = colorEntries(assist, catalog);

  function saveColor(entry: ColorEntry, color: string) {
    const result = setModelColor(assist, entry.key, color, defaultModelColor(entry.ref, catalog));
    if (result.error !== null) return result.error;
    void save("colors", { modelColors: result.modelColors }, `Color for ${entry.label} saved.`);
    return null;
  }

  function reset(entry: ColorEntry) {
    void save("colors", { modelColors: resetModelColor(assist.modelColors, entry.key) }, `Color for ${entry.label} reset to the default.`);
  }

  return (
    <section className="panel" aria-labelledby={`${ids}-title`}>
      <h3 id={`${ids}-title`}>Model colors</h3>
      <p className="hint">Each model has a color in the Text Assist ring, the Rewrite panel and History. Defaults follow the provider: OpenAI teal, Anthropic clay, Google blue, others gray.</p>
      <FeedbackText feedback={feedback} />
      <ul className="ta-color-list" aria-label="Model colors">
        {entries.map((entry) => (
          <ColorRow key={entry.key} entry={entry} busy={busy}
            color={modelColorFor(entry.ref, assist.modelColors, catalog)}
            defaultColor={defaultModelColor(entry.ref, catalog)}
            overridden={entry.key in assist.modelColors}
            onSave={(color) => saveColor(entry, color)} onReset={() => reset(entry)} />
        ))}
      </ul>
    </section>
  );
}

function ColorRow({ entry, color, defaultColor, overridden, busy, onSave, onReset }: {
  entry: ColorEntry;
  color: string;
  defaultColor: string;
  overridden: boolean;
  busy: boolean;
  onSave: (color: string) => string | null;
  onReset: () => void;
}) {
  const ids = useId();
  const [draft, setDraft] = useState(color);
  const [error, setError] = useState<string | null>(null);

  // The saved color wins whenever it changes.
  useEffect(() => { setDraft(color); setError(null); }, [color]);

  const valid = isHexColor(draft.trim());
  const changed = draft.trim().toLowerCase() !== color.toLowerCase();

  function submit() {
    const value = draft.trim();
    if (!isHexColor(value)) { setError("Enter a color as #rrggbb, for example #10a37f."); return; }
    setError(onSave(value));
  }

  return (
    <li className="ta-color-row">
      <span className="ta-favorite-name">
        <strong>{entry.label}</strong>
        <span className="ta-favorite-meta">{entry.usage}{overridden ? "" : " · default color"}</span>
      </span>
      <form className="ta-color-form" noValidate aria-label={`Color for ${entry.label}`} onSubmit={(event) => { event.preventDefault(); submit(); }}>
        <input type="color" className="ta-color-picker" value={valid ? draft.trim().toLowerCase() : color} disabled={busy}
          aria-label={`Color picker for ${entry.label}`} onChange={(event) => { setDraft(event.target.value); setError(null); }} />
        <input type="text" className="ta-color-hex" value={draft} disabled={busy} maxLength={7} spellCheck={false} autoComplete="off"
          aria-label={`Hex color for ${entry.label}`} aria-invalid={error ? true : undefined}
          aria-describedby={error ? `${ids}-error` : undefined}
          onChange={(event) => { setDraft(event.target.value); setError(null); }} />
        <button type="submit" className="icon-button" disabled={busy || !changed} aria-label={`Save color for ${entry.label}`}>Save</button>
        {overridden && (
          <button type="button" className="icon-button" disabled={busy} aria-label={`Reset color for ${entry.label}`}
            title={`Default ${defaultColor}`} onClick={onReset}>
            Reset
          </button>
        )}
      </form>
      {error && <p className="error-text ta-field-error" id={`${ids}-error`}>{error}</p>}
    </li>
  );
}
