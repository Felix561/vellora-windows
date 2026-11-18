import { useEffect, useId, useRef, useState } from "react";
import { ArrowDown, ArrowUp, Pencil, Plus, Trash2 } from "lucide-react";
import {
  ENSEMBLE_MAX_USD,
  MAX_ENSEMBLE_PROFILES,
  MAX_PROFILE_MEMBERS,
  MAX_PROFILE_NAME,
  catalogGroupLabel,
  effortLabel,
  favoriteName,
  findCatalogModel,
  fixableInProviders,
  modelAccess,
  modelAccessNote,
  modelColorFor,
  modelOptionValue,
  modelRef,
  moveFavorite,
  nextUnusedRef,
  parseEnsembleLimit,
  pickerGroups,
  pickerOptionLabel,
  profileIdFromName,
  pruneModelColors,
  refForPickerValue,
  removeFavorite,
  replaceRef,
  validateProfileDraft,
  type ProfileErrors,
} from "../lib/textAssist";
import type {
  CliBridges,
  CompatEndpoint,
  Effort,
  EnsembleProfile,
  ModelRef,
  TextAssistSettings,
  TextModelInfo,
  TextProviderKeys,
} from "../lib/types";
import "./text-models.css";

type Feedback = { text: string; error: boolean };

// ---------------------------------------------------------------------------
// Shared "model + effort" picker. Used for Quick Polish, the Rewrite panel models and ensemble
// members, so every place chooses models the same way.
// ---------------------------------------------------------------------------

/** What every picker needs to know about the catalog, saved keys and bridges. */
export type PickerContext = {
  catalog: readonly TextModelInfo[];
  endpoints: readonly CompatEndpoint[];
  keys: TextProviderKeys | null;
  cli: CliBridges;
  /** OpenAI key status when it is known from elsewhere (Settings); overrides `keys.openai`. */
  openaiKey?: boolean | null;
  loading: boolean;
  /** Opens AI Models › Text models › Providers & keys; when set, key and bridge notes get a button. */
  onShowProviders?: () => void;
};

type PickerProps = {
  value: ModelRef;
  context: PickerContext;
  /** Visible label of the model dropdown. */
  label?: string;
  /** Accessible name of the model dropdown when it differs from the visible label. */
  ariaLabel?: string;
  /** Accessible name of the effort radio group. */
  effortGroupLabel: string;
  /** Hide the "Model" and "Effort" captions (list rows); the accessible names stay. */
  compact?: boolean;
  /** Tag quick candidates as "Recommended" (Quick Polish). */
  recommend?: boolean;
  /** Offer the models of OpenAI-compatible endpoints too. */
  includeEndpoints?: boolean;
  /** Changes are ignored while busy; the controls keep focus instead of being disabled. */
  busy: boolean;
  disabled?: boolean;
  onChange: (ref: ModelRef) => void;
  selectId?: string;
};

/**
 * Compact model + effort picker: a dropdown grouped by provider (price per 1M tokens, or "plan",
 * plus key and bridge notes) and a segmented effort control that offers only that model's efforts.
 * Picking another model preselects its default effort.
 */
export function ModelEffortPicker({
  value, context, label = "Model", ariaLabel, effortGroupLabel, compact = false, recommend = false,
  includeEndpoints = true, busy, disabled = false, onChange, selectId,
}: PickerProps) {
  const ids = useId();
  const { catalog, endpoints, keys, cli, openaiKey, loading, onShowProviders } = context;
  const current = value.provider === "openai_compatible" ? null : findCatalogModel(catalog, value);
  const endpoint = value.provider === "openai_compatible" ? endpoints.find((item) => item.id === value.endpoint) : null;
  // Endpoint models count as listed only where endpoint options are offered.
  const known = Boolean(current) || Boolean(includeEndpoints && endpoint && endpoint.models.includes(value.model));
  const access = modelAccess(value, keys, cli, openaiKey);
  const note = modelAccessNote(value, access);
  const noteId = `${ids}-note`;
  const shownEndpoints = includeEndpoints ? endpoints.filter((item) => item.models.length > 0) : [];

  function pickModel(optionValue: string) {
    if (busy || disabled) return;
    const ref = refForPickerValue(optionValue, catalog, endpoints);
    if (ref) onChange(ref);
  }

  function pickEffort(effort: Effort) {
    if (busy || disabled || effort === value.effort) return;
    onChange(modelRef(value.provider, value.model, effort, value.endpoint));
  }

  return (
    <div className={`ta-picker${compact ? " ta-picker-compact" : ""}`}>
      <label className="field ta-picker-model">
        <span className={compact ? "visually-hidden" : undefined}>{label}</span>
        <select id={selectId} value={modelOptionValue(value)} disabled={disabled || (loading && !known)}
          aria-label={ariaLabel} aria-describedby={note ? noteId : undefined}
          onChange={(event) => pickModel(event.target.value)}>
          {!known && (
            <option value={modelOptionValue(value)}>
              {value.model}{loading ? "" : " (not in the model list)"}
            </option>
          )}
          {pickerGroups(catalog).map((group) => {
            const groupAccess = modelAccess({ provider: group.provider }, keys, cli, openaiKey);
            return (
              <optgroup key={group.provider} label={`${catalogGroupLabel(group.provider)}${groupAccess === "bridge_off" ? " (bridge off)" : ""}`}>
                {group.models.map((model) => (
                  <option key={model.id} value={modelOptionValue({ provider: model.provider, model: model.id })}>
                    {pickerOptionLabel(model, groupAccess, recommend)}
                  </option>
                ))}
              </optgroup>
            );
          })}
          {shownEndpoints.map((item) => (
            <optgroup key={item.id} label={`${item.name} (OpenAI-compatible)`}>
              {item.models.map((model) => (
                <option key={model} value={modelOptionValue({ provider: "openai_compatible", model, endpoint: item.id })}>{model}</option>
              ))}
            </optgroup>
          ))}
        </select>
      </label>
      <EffortSegments value={value} model={current} groupLabel={effortGroupLabel} compact={compact}
        disabled={disabled} onPick={pickEffort} />
      {note && (
        <p className="warning-text ta-access-note" id={noteId}>
          <span>{note}</span>
          {onShowProviders && fixableInProviders(value, access) && (
            <button type="button" className="icon-button ta-link-button" onClick={onShowProviders}>Show Providers &amp; keys</button>
          )}
        </p>
      )}
    </div>
  );
}

/** Segmented control (native radios in a radiogroup) with only the efforts the model offers. */
function EffortSegments({ value, model, groupLabel, compact, disabled, onPick }: {
  value: ModelRef;
  model: TextModelInfo | null;
  groupLabel: string;
  compact: boolean;
  disabled: boolean;
  onPick: (effort: Effort) => void;
}) {
  const name = useId();
  const efforts: Effort[] = model ? [...model.efforts] : [];
  // A saved effort the model no longer offers stays visible so the user sees what is stored.
  const stale = value.effort && model && !model.efforts.includes(value.effort) ? value.effort : null;
  if (stale) efforts.push(stale);

  return (
    <div className="ta-effort-field">
      {!compact && <span className="ta-effort-caption" aria-hidden="true">Effort</span>}
      {efforts.length === 0 ? (
        <span className="ta-effort-none">
          <span className="visually-hidden">{groupLabel}: </span>
          {value.provider === "openai_compatible" ? "No effort sent" : value.effort ? effortLabel(value.effort) : "Model default"}
        </span>
      ) : (
        <div className="ta-segments" role="radiogroup" aria-label={groupLabel}>
          {efforts.map((effort) => {
            const checked = value.effort === effort;
            const isDefault = model?.defaultEffort === effort;
            return (
              <label key={effort} className={`ta-segment${checked ? " checked" : ""}`}
                title={effort === stale ? "Saved effort that this model no longer offers" : isDefault ? "Model default" : undefined}>
                <input type="radio" className="ta-segment-input" name={name} value={effort} checked={checked} disabled={disabled}
                  onChange={() => onPick(effort)} />
                <span>{effortLabel(effort)}{effort === stale ? " (not offered)" : ""}</span>
              </label>
            );
          })}
        </div>
      )}
    </div>
  );
}

type ListProps = {
  refs: readonly ModelRef[];
  max: number;
  context: PickerContext;
  modelColors: Record<string, string>;
  busy: boolean;
  /** Accessible name of the list and the prefix of each row's labels, e.g. "Rewrite panel model". */
  listLabel: string;
  rowLabel: string;
  /** Suffix of the remove buttons' names, e.g. "from the Rewrite panel". */
  removeSuffix: string;
  emptyText: string;
  duplicateMessage: string;
  /** Applies a new list; `message` describes the change for the status line. */
  onChange: (refs: ModelRef[], message: string) => void;
  onError: (message: string) => void;
  idPrefix?: string;
};

/** Ordered list of models with the shared picker per row, move up/down, remove, and "Add model" up to `max`. */
export function ModelRefList({
  refs, max, context, modelColors, busy, listLabel, rowLabel, removeSuffix, emptyText, duplicateMessage, onChange, onError, idPrefix,
}: ListProps) {
  const generated = useId();
  const ids = idPrefix ?? generated;
  const pendingFocus = useRef<{ index: number; action: "up" | "down" | "remove" | "add" } | null>(null);
  const addButton = useRef<HTMLButtonElement>(null);
  const full = refs.length >= max;
  const { catalog, endpoints } = context;

  // Keep keyboard focus on the moved, removed or added row once the new list arrives.
  useEffect(() => {
    const pending = pendingFocus.current;
    if (!pending || busy) return;
    pendingFocus.current = null;
    const element = (index: number, action: string) => document.getElementById(`${ids}-row-${index}-${action}`) as HTMLButtonElement | HTMLSelectElement | null;
    if (pending.action === "add") { element(pending.index, "model")?.focus(); return; }
    if (pending.action === "remove") {
      const next = element(Math.min(pending.index, refs.length - 1), "remove");
      (next ?? addButton.current)?.focus();
      return;
    }
    const target = element(pending.index, pending.action);
    (target && !target.disabled ? target : element(pending.index, pending.action === "up" ? "down" : "up"))?.focus();
  }, [refs, busy, ids]);

  const nameOf = (ref: ModelRef) => favoriteName(ref, catalog, endpoints);

  function add() {
    if (busy || full) return;
    const ref = nextUnusedRef(refs, catalog, context.cli);
    if (!ref) { onError("Every model and effort is already in the list."); return; }
    pendingFocus.current = { index: refs.length, action: "add" };
    onChange([...refs, ref], `${nameOf(ref)} added. Choose another model or effort in the new row if you like.`);
  }

  function change(index: number, ref: ModelRef) {
    const result = replaceRef(refs, index, ref, duplicateMessage);
    if (result.error !== null) { onError(result.error); return; }
    onChange(result.refs, `${rowLabel} ${index + 1} set to ${nameOf(ref)}.`);
  }

  function move(index: number, delta: number) {
    if (busy) return;
    pendingFocus.current = { index: index + delta, action: delta < 0 ? "up" : "down" };
    onChange(moveFavorite(refs, index, delta), `${nameOf(refs[index])} moved to position ${index + delta + 1}.`);
  }

  function remove(index: number) {
    if (busy) return;
    pendingFocus.current = { index, action: "remove" };
    onChange(removeFavorite(refs, index), `${nameOf(refs[index])} removed.`);
  }

  return (
    <div className="ta-ref-list">
      {refs.length === 0 ? (
        <p className="hint">{emptyText}</p>
      ) : (
        <ol className="ta-favorites" aria-label={listLabel}>
          {refs.map((ref, index) => {
            const name = nameOf(ref);
            const position = `${rowLabel} ${index + 1}`;
            return (
              <li key={`${index}-${modelOptionValue(ref)}-${ref.effort ?? ""}`} className="ta-favorite ta-ref-row">
                <span className="ta-ref-index" aria-hidden="true">{index + 1}</span>
                <span className="ta-swatch" style={{ background: modelColorFor(ref, modelColors, catalog) }} aria-hidden="true" />
                <ModelEffortPicker value={ref} context={context} compact busy={busy} selectId={`${ids}-row-${index}-model`}
                  ariaLabel={position} effortGroupLabel={`Effort for ${position}`} onChange={(next) => change(index, next)} />
                <span className="ta-favorite-actions">
                  <button id={`${ids}-row-${index}-up`} type="button" className="icon-button" disabled={index === 0}
                    aria-label={`Move ${name} up`} title="Move up" onClick={() => move(index, -1)}>
                    <ArrowUp size={15} aria-hidden="true" />
                  </button>
                  <button id={`${ids}-row-${index}-down`} type="button" className="icon-button" disabled={index === refs.length - 1}
                    aria-label={`Move ${name} down`} title="Move down" onClick={() => move(index, 1)}>
                    <ArrowDown size={15} aria-hidden="true" />
                  </button>
                  <button id={`${ids}-row-${index}-remove`} type="button" className="icon-button danger-text"
                    aria-label={`Remove ${name} ${removeSuffix}`} title="Remove" onClick={() => remove(index)}>
                    <Trash2 size={15} aria-hidden="true" />
                  </button>
                </span>
              </li>
            );
          })}
        </ol>
      )}
      <div className="history-toolbar ta-row-actions ta-list-actions">
        <button ref={addButton} type="button" className="icon-button" disabled={full || (catalog.length === 0 && context.loading)}
          aria-describedby={`${ids}-count`} onClick={add}>
          <Plus size={15} aria-hidden="true" /> Add model
        </button>
        <span className="hint ta-list-count" id={`${ids}-count`}>
          {full ? `${refs.length} of ${max} models, the maximum. Remove one to add another.` : `${refs.length} of ${max} models`}
        </span>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Ensemble profiles
// ---------------------------------------------------------------------------

type Props = {
  assist: TextAssistSettings;
  context: PickerContext;
  busy: boolean;
  /** Saves a `textAssist` patch; resolves to false when saving failed (the caller reports it). */
  save: (patch: Partial<TextAssistSettings>, success: string) => Promise<boolean>;
  feedback: Feedback | null;
};

/** Models -> Text models -> Ensemble profiles: named lists of models the Rewrite panel can run side by side. */
export function EnsembleProfilesSection({ assist, context, busy, save, feedback }: Props) {
  const ids = useId();
  const profiles = assist.ensembleProfiles;
  const [editing, setEditing] = useState<string | "new" | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);
  const addButton = useRef<HTMLButtonElement>(null);
  const full = profiles.length >= MAX_ENSEMBLE_PROFILES;
  const { catalog, endpoints } = context;

  function closeForm() {
    setEditing(null);
    window.requestAnimationFrame(() => addButton.current?.focus());
  }

  async function submit(profile: EnsembleProfile) {
    const editingId = editing === "new" ? null : editing;
    const ensembleProfiles = editingId
      ? profiles.map((item) => (item.id === editingId ? profile : item))
      : [...profiles, profile];
    const modelColors = pruneModelColors({ ...assist, ensembleProfiles });
    const ok = await save({ ensembleProfiles, modelColors }, `${profile.name} ${editingId ? "updated" : "added"}.`);
    if (ok) closeForm();
    return ok;
  }

  async function remove(profile: EnsembleProfile) {
    const ensembleProfiles = profiles.filter((item) => item.id !== profile.id);
    const modelColors = pruneModelColors({ ...assist, ensembleProfiles });
    const ok = await save({ ensembleProfiles, modelColors }, `${profile.name} removed.`);
    if (!ok) return;
    setConfirmRemove(null);
    if (editing === profile.id) setEditing(null);
    window.requestAnimationFrame(() => addButton.current?.focus());
  }

  return (
    <section className="panel" aria-labelledby={`${ids}-title`}>
      <h3 id={`${ids}-title`}>Ensemble profiles</h3>
      <p className="hint">
        In the Rewrite panel, choose an ensemble to run every model of a profile on the same text. Each model answers in its own card,
        and you apply the one you like. A profile can list the same model with different efforts.
        Up to {MAX_ENSEMBLE_PROFILES} profiles with 1 to {MAX_PROFILE_MEMBERS} models each.
      </p>
      {feedback && <p className={feedback.error ? "error-text ta-feedback" : "success-text ta-feedback"} role={feedback.error ? "alert" : "status"}>{feedback.text}</p>}
      {profiles.length === 0 ? (
        <p className="hint">No profiles yet. The Rewrite panel then uses one profile called "Favorites": the Quick Polish model and your first Rewrite panel models, up to four models.</p>
      ) : (
        <ul className="ta-endpoint-list" aria-label="Ensemble profiles">
          {profiles.map((profile) => (
            <li key={profile.id} className="ta-endpoint">
              <div className="ta-provider-head">
                <h4>{profile.name}</h4>
                <span className="chip">{profile.members.length} model{profile.members.length === 1 ? "" : "s"}</span>
              </div>
              <ol className="ta-profile-members" aria-label={`Models in ${profile.name}`}>
                {profile.members.map((ref, index) => (
                  <li key={`${index}-${modelOptionValue(ref)}-${ref.effort ?? ""}`}>
                    <span className="ta-swatch" style={{ background: modelColorFor(ref, assist.modelColors, catalog) }} aria-hidden="true" />
                    {memberSummary(ref, catalog, endpoints)}
                  </li>
                ))}
              </ol>
              <div className="history-toolbar ta-row-actions">
                <button type="button" className="icon-button" disabled={busy} aria-label={`Edit ${profile.name}`}
                  onClick={() => { setConfirmRemove(null); setEditing(profile.id); }}>
                  <Pencil size={15} aria-hidden="true" /> Edit
                </button>
                <button type="button" className="icon-button danger-text" disabled={busy} aria-label={`Remove ${profile.name}`}
                  onClick={() => setConfirmRemove(profile.id)}>
                  <Trash2 size={15} aria-hidden="true" /> Remove
                </button>
              </div>
              {confirmRemove === profile.id && (
                <div className="inline-confirmation" role="group" aria-label={`Confirm removing ${profile.name}`}>
                  <span>Remove the ensemble profile {profile.name}?{profiles.length === 1 ? " The Rewrite panel then uses Favorites again." : ""}</span>
                  <button type="button" className="icon-button danger-text" disabled={busy} onClick={() => void remove(profile)}>Remove profile</button>
                  <button type="button" className="icon-button" disabled={busy} onClick={() => setConfirmRemove(null)}>Cancel</button>
                </div>
              )}
              {editing === profile.id && (
                <ProfileForm initial={profile} existing={profiles} context={context} modelColors={assist.modelColors}
                  busy={busy} onSubmit={submit} onCancel={closeForm} />
              )}
            </li>
          ))}
        </ul>
      )}
      {editing === "new" ? (
        <ProfileForm initial={null} existing={profiles} context={context} modelColors={assist.modelColors}
          busy={busy} onSubmit={submit} onCancel={closeForm} />
      ) : (
        <div className="history-toolbar ta-row-actions">
          <button ref={addButton} type="button" className="icon-button" disabled={busy || full}
            aria-describedby={full ? `${ids}-full` : undefined}
            onClick={() => { setConfirmRemove(null); setEditing("new"); }}>
            <Plus size={15} aria-hidden="true" /> Add profile
          </button>
        </div>
      )}
      {full && <p className="hint" id={`${ids}-full`}>Vellora supports at most {MAX_ENSEMBLE_PROFILES} profiles. Remove one to add another.</p>}
      <EnsembleLimit assist={assist} busy={busy} save={save} />
    </section>
  );
}

function memberSummary(ref: ModelRef, catalog: readonly TextModelInfo[], endpoints: readonly CompatEndpoint[]) {
  if (ref.provider === "openai_compatible") return favoriteName(ref, catalog, endpoints);
  return `${findCatalogModel(catalog, ref)?.label ?? ref.model} · ${effortLabel(ref.effort)}`;
}

function EnsembleLimit({ assist, busy, save }: Pick<Props, "assist" | "busy" | "save">) {
  const ids = useId();
  const saved = assist.limits.ensembleMaxUsdPerRun;
  const [draft, setDraft] = useState(String(saved));
  useEffect(() => { setDraft(String(saved)); }, [saved]);
  const parsed = parseEnsembleLimit(draft);
  const changed = parsed.value !== null && parsed.value !== saved;

  function submit() {
    if (parsed.value === null || !changed) return;
    void save({ limits: { ...assist.limits, ensembleMaxUsdPerRun: parsed.value } }, `Ensemble cost limit set to $${parsed.value}.`);
  }

  return (
    <form className="ta-limits ta-ensemble-limit" noValidate aria-label="Ensemble cost limit" onSubmit={(event) => { event.preventDefault(); submit(); }}>
      <label className="field">
        <span>Cost limit per ensemble run (USD)</span>
        <input type="number" inputMode="decimal" min={ENSEMBLE_MAX_USD.min} max={ENSEMBLE_MAX_USD.max} step={0.01}
          value={draft} disabled={busy}
          aria-invalid={parsed.error ? true : undefined}
          aria-describedby={parsed.error ? `${ids}-error` : `${ids}-hint`}
          onChange={(event) => setDraft(event.target.value)} />
      </label>
      {parsed.error
        ? <p className="error-text ta-field-error" id={`${ids}-error`}>{parsed.error}</p>
        : <p className="hint ta-field-hint" id={`${ids}-hint`}>The panel shows the estimated API cost before a run and blocks runs above this limit. Plan and local models count as $0.</p>}
      <div className="history-toolbar ta-limits-actions">
        <button type="submit" className="icon-button" disabled={busy || !changed}>Save limit</button>
      </div>
    </form>
  );
}

type FormProps = {
  initial: EnsembleProfile | null;
  existing: readonly EnsembleProfile[];
  context: PickerContext;
  modelColors: Record<string, string>;
  busy: boolean;
  onSubmit: (profile: EnsembleProfile) => Promise<boolean>;
  onCancel: () => void;
};

function ProfileForm({ initial, existing, context, modelColors, busy, onSubmit, onCancel }: FormProps) {
  const ids = useId();
  const [name, setName] = useState(initial?.name ?? "");
  const [members, setMembers] = useState<ModelRef[]>(() => [...(initial?.members ?? [])]);
  const [submitted, setSubmitted] = useState(false);
  const [memberError, setMemberError] = useState<string | null>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const editingId = initial?.id ?? null;

  useEffect(() => { nameInput.current?.focus(); }, []);

  const result = validateProfileDraft({ name, members }, existing, editingId);
  const errors: ProfileErrors = submitted && result.errors ? result.errors : {};
  const previewId = editingId ?? profileIdFromName(name, existing.map((profile) => profile.id));
  const title = initial ? `Edit ${initial.name}` : "Add an ensemble profile";

  async function submit() {
    setSubmitted(true);
    if (!result.profile || busy) {
      if (result.errors?.name) document.getElementById(`${ids}-name`)?.focus();
      else if (result.errors?.members) document.getElementById(`${ids}-members-title`)?.focus();
      return;
    }
    await onSubmit(result.profile);
  }

  return (
    <form className="ta-endpoint-form ta-profile-form" noValidate aria-label={title} onSubmit={(event) => { event.preventDefault(); void submit(); }}>
      <h4>{title}</h4>
      <label className="field">
        <span>Name</span>
        <input ref={nameInput} id={`${ids}-name`} type="text" maxLength={80} autoComplete="off" value={name} disabled={busy}
          aria-invalid={errors.name ? true : undefined}
          aria-describedby={errors.name ? `${ids}-name-error` : `${ids}-name-hint`}
          onChange={(event) => setName(event.target.value)} />
      </label>
      {errors.name ? <p className="error-text ta-field-error" id={`${ids}-name-error`}>{errors.name}</p>
        : <p className="hint ta-field-hint" id={`${ids}-name-hint`}>{editingId ? `ID ${editingId} (fixed).` : `ID ${previewId}, derived from the name.`} Up to {MAX_PROFILE_NAME} characters.</p>}

      <h5 className="ta-profile-subhead" id={`${ids}-members-title`} tabIndex={-1}>Models</h5>
      <ModelRefList refs={members} max={MAX_PROFILE_MEMBERS} context={context} modelColors={modelColors} busy={busy}
        listLabel={`Models in ${name.trim() || "the new profile"}`} rowLabel="Profile model" removeSuffix="from the profile"
        emptyText="No models yet. Add at least one."
        duplicateMessage="This model and effort is already in the profile."
        onChange={(next) => { setMemberError(null); setMembers(next); }}
        onError={setMemberError} />
      {errors.members && <p className="error-text ta-field-error" role="alert">{errors.members}</p>}
      {memberError && <p className="error-text ta-field-error" role="alert">{memberError}</p>}

      <div className="history-toolbar ta-row-actions">
        <button type="submit" className="icon-button" disabled={busy}>{initial ? "Save profile" : "Add profile"}</button>
        <button type="button" className="icon-button" disabled={busy} onClick={onCancel}>Cancel</button>
      </div>
    </form>
  );
}
