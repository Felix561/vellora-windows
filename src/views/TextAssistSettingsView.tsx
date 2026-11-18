import { useEffect, useId, useMemo, useState } from "react";
import { ArrowRight } from "lucide-react";
import {
  PASTE_RESTORE_DELAY_MS,
  QUICK_MAX_CHARS,
  fixableInProviders,
  isCliProvider,
  modelAccess,
  modelAccessNote,
  modelChoiceSummary,
  modelColorFor,
  useTextAssistOverview,
  useTextProviderKeys,
} from "../lib/textAssist";
import type { AppSettings, SettingsChange, TextAssistSettings } from "../lib/types";
import "./text-models.css";

type Props = {
  settings: AppSettings;
  hasKey: boolean;
  onSettingsChange: SettingsChange;
  /** Opens AI Models › Text models, where Text Assist models are chosen. Without it, the view names the place. */
  onOpenTextModels?: () => void;
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

export function TextAssistSettingsView({ settings, hasKey, onSettingsChange, onOpenTextModels }: Props) {
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
  const quickModel = assist.quickModel;
  // The OpenAI key status comes from App (it changes in Settings, General); the others from Credential Manager.
  const access = assist.devOfflineTransform ? "ok" : modelAccess(quickModel, keys, assist.cli, hasKey);
  const accessNote = modelAccessNote(quickModel, access);
  const profiles = assist.ensembleProfiles.length;
  const panelSummary = `Rewrite panel: ${assist.favoriteModels.length} model${assist.favoriteModels.length === 1 ? "" : "s"}`
    + ` · Ensembles: ${profiles === 0 ? "Favorites (automatic)" : `${profiles} profile${profiles === 1 ? "" : "s"}`}`;

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

  return (
    <div className="settings-stack">
      {message && <p className={`settings-feedback ${message.error ? "error-text" : "success-text"}`} role={message.error ? "alert" : "status"}>{message.text}</p>}
      {busy && <p className="settings-feedback" role="status">Saving settings...</p>}

      <section className="panel" aria-labelledby={`${ids}-title`}>
        <h3 id={`${ids}-title`}>Text Assist</h3>
        <label className="toggle-row">
          <input type="checkbox" checked={assist.enabled} disabled={busy}
            onChange={(event) => void save({ enabled: event.target.checked })} />
          <span>Enable Text Assist</span>
        </label>
        <div className="hint ta-usage" id={`${ids}-usage`}>
          <p><strong>Quick Polish:</strong> select text in any app and tap <kbd>Shift</kbd>+<kbd>Win</kbd> to fix spelling, grammar and punctuation in place.</p>
          <p><strong>Rewrite panel:</strong></p>
          <ol className="ta-steps" aria-label="Rewrite panel steps">
            <li>Select text and hold <kbd>Shift</kbd>+<kbd>Win</kbd> to open the panel. Nothing is sent yet.</li>
            <li>Pick a style and a model, or an ensemble.</li>
            <li>Tap <kbd>Shift</kbd>+<kbd>Win</kbd> to start.</li>
            <li>Press <kbd>Enter</kbd> to apply the result, or <kbd>Esc</kbd> to close.</li>
          </ol>
        </div>
      </section>

      <section className="panel" aria-labelledby={`${ids}-model-title`}>
        <h3 id={`${ids}-model-title`}>Models</h3>
        <div className="ta-model-summary">
          <span className="ta-swatch" style={{ background: modelColorFor(quickModel, assist.modelColors, models) }} aria-hidden="true" />
          <p className="ta-model-summary-text" id={`${ids}-quick-summary`}>
            <strong>Quick Polish:</strong>{" "}
            {!overview && !modelsError ? "Loading the model list..." : modelChoiceSummary(quickModel, models, assist.openaiCompatible)}
            <span className="ta-model-summary-meta">{panelSummary}</span>
          </p>
          {onOpenTextModels && (
            <button type="button" className="icon-button" aria-describedby={`${ids}-quick-summary`} onClick={onOpenTextModels}>
              Choose models <ArrowRight size={15} aria-hidden="true" />
            </button>
          )}
        </div>
        <p className="hint ta-key-note">
          {onOpenTextModels ? "Models and efforts for Quick Polish, the Rewrite panel and ensembles are chosen in AI Models › Text models."
            : "Choose models and efforts for Quick Polish, the Rewrite panel and ensembles in AI Models › Text models."}
          {isCliProvider(quickModel.provider) && " Plan models take several seconds; not recommended for Quick Polish."}
        </p>
        {accessNote && (
          <p className="warning-text ta-access-note" role="note">
            <span>Quick Polish cannot run yet. {accessNote}</span>
            {onOpenTextModels && fixableInProviders(quickModel, access) && (
              <button type="button" className="icon-button ta-link-button" onClick={onOpenTextModels}>Open AI Models</button>
            )}
          </p>
        )}
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
