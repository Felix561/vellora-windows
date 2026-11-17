import { useEffect, useId, useMemo, useRef, useState } from "react";
import { ArrowDown, ArrowUp, Pencil, Plus, Trash2 } from "lucide-react";
import {
  ENSEMBLE_MAX_USD,
  MAX_ENSEMBLE_PROFILES,
  MAX_PROFILE_MEMBERS,
  MAX_PROFILE_NAME,
  addProfileMember,
  catalogOptionLabel,
  effortForModel,
  effortLabel,
  favoriteName,
  findCatalogModel,
  groupCatalogModels,
  modelColorFor,
  modelOptionValue,
  modelRef,
  modelRefLabel,
  moveFavorite,
  needsKey,
  parseEnsembleLimit,
  parseModelOptionValue,
  profileIdFromName,
  providerLabel,
  pruneModelColors,
  removeFavorite,
  setMemberEffort,
  validateProfileDraft,
  type ProfileErrors,
} from "../lib/textAssist";
import type {
  CompatEndpoint,
  Effort,
  EnsembleProfile,
  ModelRef,
  TextAssistSettings,
  TextModelInfo,
  TextProviderKeys,
} from "../lib/types";

type Feedback = { text: string; error: boolean };

type Props = {
  assist: TextAssistSettings;
  catalog: readonly TextModelInfo[];
  loading: boolean;
  keys: TextProviderKeys | null;
  busy: boolean;
  /** Saves a `textAssist` patch; resolves to false when saving failed (the caller reports it). */
  save: (patch: Partial<TextAssistSettings>, success: string) => Promise<boolean>;
  feedback: Feedback | null;
};

/** Grouped catalog and endpoint options for a model `<select>` (value: `modelOptionValue`). */
export function ModelOptions({ catalog, endpoints, keys }: {
  catalog: readonly TextModelInfo[];
  endpoints: readonly CompatEndpoint[];
  keys: TextProviderKeys | null;
}) {
  const groups = useMemo(() => groupCatalogModels(catalog), [catalog]);
  return (
    <>
      {groups.map((group) => (
        <optgroup key={group.provider} label={providerLabel(group.provider)}>
          {group.models.map((model) => (
            <option key={model.id} value={modelOptionValue({ provider: model.provider, model: model.id })}>
              {catalogOptionLabel(model, needsKey(model, keys))}
            </option>
          ))}
        </optgroup>
      ))}
      {endpoints.filter((endpoint) => endpoint.models.length > 0).map((endpoint) => (
        <optgroup key={endpoint.id} label={`${endpoint.name} (OpenAI-compatible)`}>
          {endpoint.models.map((model) => (
            <option key={model} value={modelOptionValue({ provider: "openai_compatible", model, endpoint: endpoint.id })}>{model}</option>
          ))}
        </optgroup>
      ))}
    </>
  );
}

function memberSummary(ref: ModelRef, catalog: readonly TextModelInfo[], endpoints: readonly CompatEndpoint[]) {
  if (ref.provider === "openai_compatible") return favoriteName(ref, catalog, endpoints);
  return `${modelRefLabel(ref, catalog)} · ${effortLabel(ref.effort)}`;
}

/** Models -> Text models -> Ensemble profiles: named lists of models for the Rewrite panel's ensemble (E). */
export function EnsembleProfilesSection({ assist, catalog, loading, keys, busy, save, feedback }: Props) {
  const ids = useId();
  const profiles = assist.ensembleProfiles;
  const endpoints = assist.openaiCompatible;
  const [editing, setEditing] = useState<string | "new" | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);
  const addButton = useRef<HTMLButtonElement>(null);
  const full = profiles.length >= MAX_ENSEMBLE_PROFILES;

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
        In the Rewrite panel, <kbd>E</kbd> runs every model of a profile on the same text. Each model answers in its own card;
        pick one with <kbd>1</kbd>–<kbd>9</kbd> and press <kbd>Enter</kbd>. A profile can list the same model with different efforts.
        Up to {MAX_ENSEMBLE_PROFILES} profiles with 1 to {MAX_PROFILE_MEMBERS} models each.
      </p>
      {feedback && <p className={feedback.error ? "error-text ta-feedback" : "success-text ta-feedback"} role={feedback.error ? "alert" : "status"}>{feedback.text}</p>}
      {profiles.length === 0 ? (
        <p className="hint">No profiles yet. The Rewrite panel then uses one profile called "Favorites": the Quick Polish model and your first favorites, up to four models.</p>
      ) : (
        <ul className="ta-endpoint-list" aria-label="Ensemble profiles">
          {profiles.map((profile) => (
            <li key={profile.id} className="ta-endpoint">
              <div className="ta-provider-head">
                <h4>{profile.name}</h4>
                <span className="chip">{profile.members.length} model{profile.members.length === 1 ? "" : "s"}</span>
              </div>
              <p className="hint ta-endpoint-meta">ID {profile.id}</p>
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
                <ProfileForm initial={profile} existing={profiles} catalog={catalog} loading={loading} endpoints={endpoints}
                  keys={keys} modelColors={assist.modelColors} busy={busy} onSubmit={submit} onCancel={closeForm} />
              )}
            </li>
          ))}
        </ul>
      )}
      {editing === "new" ? (
        <ProfileForm initial={null} existing={profiles} catalog={catalog} loading={loading} endpoints={endpoints}
          keys={keys} modelColors={assist.modelColors} busy={busy} onSubmit={submit} onCancel={closeForm} />
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
  catalog: readonly TextModelInfo[];
  loading: boolean;
  endpoints: readonly CompatEndpoint[];
  keys: TextProviderKeys | null;
  modelColors: Record<string, string>;
  busy: boolean;
  onSubmit: (profile: EnsembleProfile) => Promise<boolean>;
  onCancel: () => void;
};

function ProfileForm({ initial, existing, catalog, loading, endpoints, keys, modelColors, busy, onSubmit, onCancel }: FormProps) {
  const ids = useId();
  const [name, setName] = useState(initial?.name ?? "");
  const [members, setMembers] = useState<ModelRef[]>(() => [...(initial?.members ?? [])]);
  const [choice, setChoice] = useState("");
  const [effort, setEffort] = useState<Effort | "">("");
  const [submitted, setSubmitted] = useState(false);
  const [memberError, setMemberError] = useState<string | null>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const editingId = initial?.id ?? null;
  const full = members.length >= MAX_PROFILE_MEMBERS;

  useEffect(() => { nameInput.current?.focus(); }, []);

  const result = validateProfileDraft({ name, members }, existing, editingId);
  const errors: ProfileErrors = submitted && result.errors ? result.errors : {};
  const previewId = editingId ?? profileIdFromName(name, existing.map((profile) => profile.id));
  const title = initial ? `Edit ${initial.name}` : "Add an ensemble profile";

  const chosen = parseModelOptionValue(choice);
  const chosenModel = chosen && chosen.provider !== "openai_compatible" ? findCatalogModel(catalog, chosen) : null;
  const chosenEfforts = chosenModel?.efforts ?? [];

  function selectChoice(value: string) {
    setChoice(value);
    setMemberError(null);
    const parsed = parseModelOptionValue(value);
    const model = parsed && parsed.provider !== "openai_compatible" ? findCatalogModel(catalog, parsed) : null;
    setEffort(model ? (effortForModel(model, effort || null) ?? "") : "");
  }

  function addMember() {
    if (!chosen) return;
    if (chosen.provider !== "openai_compatible" && !chosenModel) return;
    const next = addProfileMember(members, modelRef(chosen.provider, chosen.model, effort || null, chosen.endpoint));
    if (next.error !== null) { setMemberError(next.error); return; }
    setMemberError(null);
    setMembers(next.members);
  }

  function changeEffort(index: number, value: Effort) {
    const next = setMemberEffort(members, index, value, findCatalogModel(catalog, members[index]));
    if (next.error !== null) { setMemberError(next.error); return; }
    setMemberError(null);
    setMembers(next.members);
  }

  function move(index: number, delta: number) {
    setMembers(moveFavorite(members, index, delta));
    // Keep focus on the same control of the moved model; fall back to its other move button at the ends.
    window.requestAnimationFrame(() => {
      const action = delta < 0 ? "up" : "down";
      const button = (which: string) => document.getElementById(`${ids}-member-${index + delta}-${which}`) as HTMLButtonElement | null;
      const target = button(action);
      (target && !target.disabled ? target : button(action === "up" ? "down" : "up"))?.focus();
    });
  }

  async function submit() {
    setSubmitted(true);
    if (!result.profile || busy) {
      const firstInvalid = (["name", "members"] as const).find((field) => result.errors?.[field]);
      if (firstInvalid) document.getElementById(`${ids}-${firstInvalid === "name" ? "name" : "model"}`)?.focus();
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

      <h5 className="ta-profile-subhead" id={`${ids}-members-title`}>Models ({members.length} of {MAX_PROFILE_MEMBERS})</h5>
      {members.length === 0 ? (
        <p className="hint">No models yet. Add at least one below.</p>
      ) : (
        <ol className="ta-favorites" aria-labelledby={`${ids}-members-title`}>
          {members.map((ref, index) => {
            const label = favoriteName(ref, catalog, endpoints);
            const model = ref.provider === "openai_compatible" ? null : findCatalogModel(catalog, ref);
            const efforts = model?.efforts ?? [];
            return (
              <li key={`${index}-${modelOptionValue(ref)}-${ref.effort ?? ""}`} className="ta-favorite">
                <span className="ta-swatch" style={{ background: modelColorFor(ref, modelColors, catalog) }} aria-hidden="true" />
                <span className="ta-favorite-name">
                  <strong>{index + 1}. {modelRefLabel(ref, catalog)}</strong>
                  <span className="ta-favorite-meta">
                    {ref.provider === "openai_compatible"
                      ? `${endpoints.find((item) => item.id === ref.endpoint)?.name ?? ref.endpoint ?? "Unknown endpoint"} (OpenAI-compatible)`
                      : providerLabel(ref.provider)}
                    {needsKey(ref, keys) && <span className="warning-text ta-needs-key"> · needs key</span>}
                  </span>
                </span>
                {efforts.length > 0 ? (
                  <select className="ta-effort" aria-label={`Effort for ${label}`} value={ref.effort ?? ""} disabled={busy}
                    onChange={(event) => changeEffort(index, event.target.value as Effort)}>
                    {!ref.effort && <option value="">Model default</option>}
                    {ref.effort && !efforts.includes(ref.effort) && <option value={ref.effort}>{effortLabel(ref.effort)} (not offered)</option>}
                    {efforts.map((item) => <option key={item} value={item}>{effortLabel(item)}</option>)}
                  </select>
                ) : (
                  <span className="ta-effort ta-effort-none">{ref.provider === "openai_compatible" ? "No effort sent" : ref.effort ? effortLabel(ref.effort) : "Default effort"}</span>
                )}
                <span className="ta-favorite-actions">
                  <button id={`${ids}-member-${index}-up`} type="button" className="icon-button" disabled={busy || index === 0}
                    aria-label={`Move ${label} up`} title="Move up" onClick={() => move(index, -1)}>
                    <ArrowUp size={15} aria-hidden="true" />
                  </button>
                  <button id={`${ids}-member-${index}-down`} type="button" className="icon-button" disabled={busy || index === members.length - 1}
                    aria-label={`Move ${label} down`} title="Move down" onClick={() => move(index, 1)}>
                    <ArrowDown size={15} aria-hidden="true" />
                  </button>
                  <button type="button" className="icon-button danger-text" disabled={busy}
                    aria-label={`Remove ${label} from the profile`} title="Remove"
                    onClick={() => { setMemberError(null); setMembers(removeFavorite(members, index)); }}>
                    <Trash2 size={15} aria-hidden="true" />
                  </button>
                </span>
              </li>
            );
          })}
        </ol>
      )}
      {errors.members && <p className="error-text ta-field-error" role="alert">{errors.members}</p>}

      <div className="ta-add-favorite" role="group" aria-label="Add a model to the profile">
        <label className="field">
          <span>Model</span>
          <select id={`${ids}-model`} value={choice} disabled={busy || full} onChange={(event) => selectChoice(event.target.value)}>
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
        <button type="button" className="icon-button" disabled={busy || full || !chosen} onClick={addMember}>
          <Plus size={15} aria-hidden="true" /> Add model
        </button>
      </div>
      {memberError && <p className="error-text ta-field-error" role="alert">{memberError}</p>}
      {full && <p className="hint">This profile has {MAX_PROFILE_MEMBERS} models, the maximum.</p>}

      <div className="history-toolbar ta-row-actions">
        <button type="submit" className="icon-button" disabled={busy}>{initial ? "Save profile" : "Add profile"}</button>
        <button type="button" className="icon-button" disabled={busy} onClick={onCancel}>Cancel</button>
      </div>
    </form>
  );
}
