import { useEffect, useId, useMemo, useState } from "react";
import { Check } from "lucide-react";
import {
  PASTE_RESTORE_DELAY_MS,
  QUICK_MAX_CHARS,
  catalogOptionLabel,
  effortForModel,
  effortLabel,
  findCatalogModel,
  groupCatalogModels,
  modelOptionValue,
  modelRef,
  needsKey,
  parseModelOptionValue,
  providerLabel,
  useTextAssistOverview,
  useTextProviderKeys,
} from "../lib/textAssist";
import type { AppSettings, Effort, SettingsChange, TextAssistSettings, TextProviderKind } from "../lib/types";

type Props = {
  settings: AppSettings;
  hasKey: boolean;
  onSettingsChange: SettingsChange;
};

/** Returns a validation message, or null when `draft` is a whole number within the range. */
export function validateLimit(draft: string, range: { min: number; max: number }, unit: string) {
  const text = draft.trim();
  const value = Number(text);
  if (!/^\d+$/.test(text) || !Number.isSafeInteger(value) || value < range.min || value > range.max) {
    return `Enter a whole number from ${range.min.toLocaleString("en-US")} to ${range.max.toLocaleString("en-US")} ${unit}.`;
  }
  return null;
}

const NO_KEY_ENDPOINTS: string[] = [];

export function TextAssistSettingsView({ settings, hasKey, onSettingsChange }: Props) {
  const assist = settings.textAssist;
  const { overview, error: modelsError } = useTextAssistOverview();
  const { keys } = useTextProviderKeys(NO_KEY_ENDPOINTS);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);
  const [quickMaxDraft, setQuickMaxDraft] = useState(String(assist.limits.quickMaxChars));
  const [delayDraft, setDelayDraft] = useState(String(assist.limits.pasteRestoreDelayMs));
  const ids = useId();

  // Saved values win over drafts whenever the confirmed settings change.
  useEffect(() => { setQuickMaxDraft(String(assist.limits.quickMaxChars)); }, [assist.limits.quickMaxChars]);
  useEffect(() => { setDelayDraft(String(assist.limits.pasteRestoreDelayMs)); }, [assist.limits.pasteRestoreDelayMs]);

  const models = useMemo(() => overview?.models ?? [], [overview]);
  const groups = useMemo(() => groupCatalogModels(models), [models]);
  const quickModel = assist.quickModel;
  const current = findCatalogModel(models, quickModel);
  // The OpenAI key status comes from App (it changes in Settings, General); the others from Credential Manager.
  const missingKey = (provider: TextProviderKind) => (provider === "openai" ? !hasKey : needsKey({ provider }, keys));
  const efforts: Effort[] = current ? current.efforts : quickModel.effort ? [quickModel.effort] : [];

  const quickMaxError = validateLimit(quickMaxDraft, QUICK_MAX_CHARS, "characters");
  const delayError = validateLimit(delayDraft, PASTE_RESTORE_DELAY_MS, "milliseconds");
  const limitsChanged = quickMaxDraft.trim() !== String(assist.limits.quickMaxChars)
    || delayDraft.trim() !== String(assist.limits.pasteRestoreDelayMs);

  async function save(patch: Partial<TextAssistSettings>) {
    if (busy) return;
    setBusy(true);
    setMessage(null);
    try {
      await onSettingsChange({ textAssist: { ...assist, ...patch } });
      setMessage({ text: "Text Assist settings saved.", error: false });
    } catch (error) {
      setMessage({ text: `Could not save Text Assist settings: ${String(error)}`, error: true });
    } finally {
      setBusy(false);
    }
  }

  function selectModel(value: string) {
    const parsed = parseModelOptionValue(value);
    const model = parsed ? findCatalogModel(models, parsed) : null;
    if (!model) return;
    void save({ quickModel: modelRef(model.provider, model.id, effortForModel(model, quickModel.effort)) });
  }

  function saveLimits() {
    if (quickMaxError || delayError || !limitsChanged) return;
    void save({
      limits: {
        ...assist.limits,
        quickMaxChars: Number(quickMaxDraft.trim()),
        pasteRestoreDelayMs: Number(delayDraft.trim()),
      },
    });
  }

  const quickNeedsKey = !assist.devOfflineTransform && missingKey(quickModel.provider);
  const quickHasKey = quickModel.provider === "openai" ? hasKey : keys ? !needsKey(quickModel, keys) : false;

  return (
    <div className="settings-stack">
      {message && <p className={`settings-feedback ${message.error ? "error-text" : "success-text"}`} role={message.error ? "alert" : "status"}>{message.text}</p>}
      {busy && <p className="settings-feedback" role="status">Saving settings...</p>}

      <section className="panel" aria-labelledby={`${ids}-title`}>
        <div className="section-title">
          <div>
            <h3 id={`${ids}-title`}>Text Assist</h3>
            <p>{quickHasKey
              ? `Uses the ${providerLabel(quickModel.provider)} key stored in Windows Credential Manager`
              : `Requires a key for ${providerLabel(quickModel.provider)}`}</p>
          </div>
          {quickHasKey && <Check size={18} aria-hidden="true" />}
        </div>
        <label className="toggle-row">
          <input type="checkbox" checked={assist.enabled} disabled={busy}
            onChange={(event) => void save({ enabled: event.target.checked })} />
          <span>Enable Text Assist</span>
        </label>
        <p className="hint">
          Select text in any app and tap <kbd>Shift</kbd>+<kbd>Win</kbd> to fix spelling, grammar and punctuation in place.
          Hold <kbd>Shift</kbd>+<kbd>Win</kbd> for the Rewrite panel (coming soon).
        </p>
        {quickNeedsKey && (
          <p className="warning-text ta-key-note">
            {quickModel.provider === "openai"
              ? "Quick Polish needs an OpenAI API key. Add one under Settings, General, OpenAI key. The same key is used for dictation."
              : `Quick Polish needs an API key for ${providerLabel(quickModel.provider)}. Add one under AI Models, Text models.`}
          </p>
        )}
      </section>

      <section className="panel" aria-labelledby={`${ids}-model-title`}>
        <h3 id={`${ids}-model-title`}>Quick Polish</h3>
        <label className="field">
          <span>Model</span>
          <select value={modelOptionValue(quickModel)} disabled={busy || models.length === 0}
            aria-describedby={`${ids}-model-hint`}
            onChange={(event) => selectModel(event.target.value)}>
            {!current && <option value={modelOptionValue(quickModel)}>{quickModel.model}{overview ? " (not in the model list)" : ""}</option>}
            {groups.map((group) => (
              <optgroup key={group.provider} label={providerLabel(group.provider)}>
                {group.models.map((model) => (
                  <option key={model.id} value={modelOptionValue({ provider: model.provider, model: model.id })}>
                    {catalogOptionLabel(model, missingKey(model.provider))}{model.quickCandidate ? " (recommended)" : ""}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Reasoning effort</span>
          <select value={quickModel.effort ?? ""} disabled={busy || !current || efforts.length === 0}
            onChange={(event) => void save({ quickModel: { ...quickModel, effort: event.target.value as Effort } })}>
            {!quickModel.effort && <option value="">Model default</option>}
            {efforts.map((effort) => <option key={effort} value={effort}>{effortLabel(effort)}</option>)}
          </select>
        </label>
        <p className="hint" id={`${ids}-model-hint`}>
          {!overview && !modelsError ? "Loading the model list..."
            : current?.shutdown ? `${current.label} retires on ${current.shutdown}. Choose another model before then.`
              : "Recommended models are fast and inexpensive. Prices are provider list prices per million tokens. Lower effort answers faster; Quick Polish rarely needs more than low."}
        </p>
        {modelsError && <p className="error-text" role="alert">Could not load the model list: {modelsError}</p>}
      </section>

      <section className="panel" aria-labelledby={`${ids}-history-title`}>
        <h3 id={`${ids}-history-title`}>History and limits</h3>
        <label className="toggle-row">
          <input type="checkbox" checked={assist.saveHistory} disabled={busy}
            onChange={(event) => void save({ saveHistory: event.target.checked })} />
          <span>Save Text Assist history</span>
        </label>
        <p className="hint">History keeps the original and the result of each run on this computer. When it is off, only the latest kept result stays available until you quit Vellora.</p>
        <form className="ta-limits" noValidate onSubmit={(event) => { event.preventDefault(); saveLimits(); }}>
          <label className="field">
            <span>Quick Polish max characters</span>
            <input type="number" inputMode="numeric" min={QUICK_MAX_CHARS.min} max={QUICK_MAX_CHARS.max} step={1}
              value={quickMaxDraft} disabled={busy}
              aria-invalid={quickMaxError ? true : undefined}
              aria-describedby={quickMaxError ? `${ids}-quick-max-error` : undefined}
              onChange={(event) => setQuickMaxDraft(event.target.value)} />
          </label>
          {quickMaxError && <p className="error-text ta-field-error" id={`${ids}-quick-max-error`}>{quickMaxError}</p>}
          <label className="field">
            <span>Paste restore delay (ms)</span>
            <input type="number" inputMode="numeric" min={PASTE_RESTORE_DELAY_MS.min} max={PASTE_RESTORE_DELAY_MS.max} step={1}
              value={delayDraft} disabled={busy}
              aria-invalid={delayError ? true : undefined}
              aria-describedby={delayError ? `${ids}-delay-error` : `${ids}-delay-hint`}
              onChange={(event) => setDelayDraft(event.target.value)} />
          </label>
          {delayError && <p className="error-text ta-field-error" id={`${ids}-delay-error`}>{delayError}</p>}
          <p className="hint" id={`${ids}-delay-hint`}>How long Vellora waits after pasting before it restores your previous clipboard.</p>
          <div className="history-toolbar ta-limits-actions">
            <button type="submit" className="icon-button" disabled={busy || !limitsChanged || Boolean(quickMaxError || delayError)}>Save limits</button>
          </div>
        </form>
      </section>

      <section className="panel" aria-labelledby={`${ids}-privacy-title`}>
        <h3 id={`${ids}-privacy-title`}>Privacy</h3>
        <p className="hint">
          Only the selected text is sent, to the provider you chose. Vellora's own clipboard writes are excluded from Win+V history;
          your editor's copy of the selection may still appear there.
        </p>
        <details className="ta-advanced">
          <summary>Advanced</summary>
          <label className="toggle-row">
            <input type="checkbox" checked={assist.devOfflineTransform} disabled={busy}
              onChange={(event) => void save({ devOfflineTransform: event.target.checked })} />
            <span>Offline test transform (uppercase, no network)</span>
          </label>
          <p className="hint">For developers: replaces the selection with uppercase text instead of contacting a provider.</p>
        </details>
      </section>
    </div>
  );
}
