import { useEffect, useState } from "react";
import { Check, KeyRound, Trash2 } from "lucide-react";
import {
  getStartAtLogin,
  setStartAtLogin,
  deleteOpenAiApiKey,
  saveOpenAiApiKey,
} from "../lib/api";
import { AppearanceView } from "./AppearanceView";
import { BRAND } from "../lib/brand";
import type { AppSettings, SettingsChange, SttModel } from "../lib/types";

type Props = {
  settings: AppSettings;
  hasKey: boolean;
  onSettingsChange: SettingsChange;
  onKeyStatusChange: (hasKey: boolean) => void;
};

const sttModels: Array<{ value: SttModel; label: string; hint: string }> = [
  {
    value: "whisper-1",
    label: "Whisper 1",
    hint: "Legacy transcription model. GPT-4o mini Transcribe costs about half as much.",
  },
  {
    value: "gpt-4o-mini-transcribe",
    label: "GPT-4o mini Transcribe",
    hint: "Lower cost GPT transcription. Compare its accuracy on your recordings.",
  },
  {
    value: "gpt-4o-transcribe",
    label: "GPT-4o Transcribe",
    hint: "Higher accuracy option for harder audio.",
  },
  {
    value: "gpt-4o-transcribe-diarize",
    label: "GPT-4o Transcribe Diarize",
    hint: "Uses the diarization model. Vellora currently displays plain text without speaker labels.",
  },
];

export function SettingsView({ settings, hasKey, onSettingsChange, onKeyStatusChange }: Props) {
  const [tab, setTab] = useState<"general" | "appearance">("general");
  const [draftKey, setDraftKey] = useState("");
  const [saving, setSaving] = useState(false);
  const [settingsBusy, setSettingsBusy] = useState(false);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);

  const [startAtLogin, setStartup] = useState<boolean | null>(null);
  const [startupBusy, setStartupBusy] = useState(false);
  const [startupError, setStartupError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    getStartAtLogin().then((enabled) => {
      if (active) setStartup(enabled);
    }).catch((error) => {
      if (active) setStartupError(String(error));
    });
    return () => { active = false; };
  }, []);

  async function updateStartup(enabled: boolean) {
    setStartupBusy(true);
    setStartupError(null);
    try {
      setStartup(await setStartAtLogin(enabled));
    } catch (error) {
      setStartupError(String(error));
    } finally {
      setStartupBusy(false);
    }
  }

  async function updateSettings(patch: Partial<AppSettings>) {
    if (settingsBusy) return;
    setSettingsBusy(true);
    setMessage(null);
    try {
      await onSettingsChange(patch);
      setMessage({ text: "Settings saved.", error: false });
    } catch (error) {
      setMessage({ text: `Could not save settings: ${String(error)}`, error: true });
    } finally { setSettingsBusy(false); }
  }

  async function saveKey() {
    if (!draftKey.trim()) return;
    setSaving(true);
    setMessage(null);
    try {
      await saveOpenAiApiKey(draftKey.trim());
      setDraftKey("");
      onKeyStatusChange(true);
      setMessage({ text: "API key saved.", error: false });
    } catch (error) {
      setMessage({ text: `Could not save API key: ${String(error)}`, error: true });
    } finally {
      setSaving(false);
    }
  }

  async function removeKey() {
    setSaving(true);
    setMessage(null);
    try {
      await deleteOpenAiApiKey();
      onKeyStatusChange(false);
      setMessage({ text: "API key removed.", error: false });
    } catch (error) {
      setMessage({ text: `Could not remove API key: ${String(error)}`, error: true });
    } finally { setSaving(false); }
  }

  return (
    <div className="settings-stack">
      <div className="settings-tabs" role="tablist" aria-label="Settings sections">
        {(["general", "appearance"] as const).map((item) => (
          <button key={item} id={`settings-tab-${item}`} role="tab"
            aria-selected={tab === item} aria-controls={`settings-panel-${item}`}
            tabIndex={tab === item ? 0 : -1}
            onClick={() => setTab(item)}
            onKeyDown={(event) => {
              if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
                event.preventDefault();
                const next = event.key === "Home" ? "general" : event.key === "End" ? "appearance" : tab === "general" ? "appearance" : "general";
                setTab(next);
                document.getElementById(`settings-tab-${next}`)?.focus();
              }
            }}>
            {item === "general" ? "General" : "Appearance"}
          </button>
        ))}
      </div>
      {tab === "appearance" ? (
        <div role="tabpanel" id="settings-panel-appearance" aria-labelledby="settings-tab-appearance">
          <AppearanceView settings={settings} onSettingsChange={onSettingsChange} />
        </div>
      ) : (
      <div className="settings-stack" role="tabpanel" id="settings-panel-general" aria-labelledby="settings-tab-general">
      {message && <p className={`settings-feedback ${message.error ? "error-text" : "success-text"}`} role={message.error ? "alert" : "status"}>{message.text}</p>}
      {settingsBusy && <p className="settings-feedback" role="status">Saving settings...</p>}
      <section className="panel">
        <div className="section-title">
          <div>
            <h3>OpenAI key</h3>
            <p>{hasKey ? "Stored in Windows Credential Manager" : "Required before dictation works"}</p>
          </div>
          {hasKey && <Check size={18} />}
        </div>
        <form className="input-row" onSubmit={(event) => { event.preventDefault(); void saveKey(); }}>
          <KeyRound size={17} aria-hidden="true" />
          <input
            value={draftKey}
            onChange={(event) => setDraftKey(event.target.value)}
            placeholder={hasKey ? "Replace saved API key" : "Paste OpenAI API key"}
            type="password"
            aria-label={hasKey ? "Replace OpenAI API key" : "OpenAI API key"}
            autoComplete="off"
            spellCheck={false}
            disabled={saving}
          />
          <button type="submit" disabled={saving || !draftKey.trim()}>
            {saving ? "Saving..." : "Save"}
          </button>
          {hasKey && (
            <button type="button" className="danger" onClick={() => void removeKey()} disabled={saving} title="Remove saved API key" aria-label="Remove saved API key">
              <Trash2 size={15} aria-hidden="true" />
            </button>
          )}
        </form>
      </section>

      <section className="panel">
        <h3>Transcription</h3>
        <label className="field">
          <span>Model</span>
          <select
            value={settings.sttModel}
            disabled={settingsBusy}
            onChange={(event) =>
              updateSettings({ sttModel: event.target.value as SttModel })
            }
          >
            {sttModels.map((model) => (
              <option key={model.value} value={model.value}>
                {model.label}
              </option>
            ))}
          </select>
        </label>
        <p className="hint">
          {sttModels.find((model) => model.value === settings.sttModel)?.hint}
        </p>
        <p className="hint">Recorded audio is sent to OpenAI when you release the hotkey.</p>
      </section>

      <section className="panel">
        <h3>Cleanup</h3>
        <label className="toggle-row">
          <input
            type="checkbox"
            checked={settings.cleanupEnabled}
            disabled={settingsBusy}
            onChange={(event) => updateSettings({ cleanupEnabled: event.target.checked })}
          />
          <span>Light cleanup after transcription</span>
        </label>
        <div className="kv compact">
          <span>Cleanup model</span>
          <strong>{settings.cleanupModel}</strong>
          <span>Mode</span>
          <strong>{settings.cleanupEnabled ? "Light" : "Off"}</strong>
        </div>
      </section>

      <section className="panel">
        <h3>Windows startup</h3>
        <label className="toggle-row">
          <input
            type="checkbox"
            checked={startAtLogin ?? false}
            disabled={startAtLogin === null || startupBusy}
            onChange={(event) => updateStartup(event.target.checked)}
          />
          <span>Start Vellora when I sign in to Windows</span>
        </label>
        <p className="hint">Starts quietly in the system tray. Changes apply immediately.</p>
        {startupError && <p role="alert">Could not update Windows startup: {startupError}</p>}
      </section>

      <section className="panel">
        <h3>Controls</h3>
        <label className="toggle-row">
          <input type="checkbox" checked={settings.autoPaste} disabled={settingsBusy}
            onChange={(event) => void updateSettings({ autoPaste: event.target.checked })} />
          <span>Automatically paste into the focused app</span>
        </label>
        <label className="toggle-row">
          <input
            type="checkbox"
            checked={settings.copyToClipboard}
            disabled={settingsBusy}
            onChange={(event) => updateSettings({ copyToClipboard: event.target.checked })}
          />
          <span>{settings.autoPaste ? "Keep transcript on clipboard after pasting" : "Copy transcript to clipboard"}</span>
        </label>
        <div className="kv compact">
          <span>Hotkey</span>
          <strong>Hold Ctrl + Win</strong>
          <span>Output</span>
          <strong>{settings.autoPaste ? settings.copyToClipboard ? "Auto-paste and clipboard" : "Auto-paste only" : settings.copyToClipboard ? "Clipboard only" : "Manual copy"}</strong>
          <span>History</span>
          <strong>{settings.saveHistory ? "Saved locally as text" : "Off"}</strong>
          <span>Overlay</span>
          <strong>Top-center status bar</strong>
          <span>Audio</span>
          <strong>Temporary, then deleted</strong>
          <span>API key</span>
          <strong>Windows Credential Manager</strong>
          <span>Storage</span>
          <strong>%APPDATA%\Vellora</strong>
        </div>
        <p className="hint">
          When clipboard saving is off, Vellora briefly uses the clipboard to paste and then restores
          the previous text. If the clipboard contains images or files, automatic paste is skipped to preserve them; the transcript stays available on the Dashboard. If paste fails, the transcript also remains available for manual copying. Your API key is stored in Windows Credential Manager. Audio is
          temporary and deleted after transcription. Existing {BRAND.oldAppName}
          data is copied into {` ${BRAND.appName}`} on first launch and kept as a backup.
        </p>
      </section>

      <section className="panel">
        <h3>Privacy and history</h3>
        <label className="toggle-row">
          <input type="checkbox" checked={settings.saveHistory} disabled={settingsBusy}
            onChange={(event) => void updateSettings({ saveHistory: event.target.checked })} />
          <span>Save transcript history on this computer</span>
        </label>
        <p className="hint">History stores the original transcription and the final text locally. Turning this off affects future dictations; you can delete existing transcripts in History. The latest transcript stays available for manual copy until you dictate again or quit.</p>
        <p className="hint">Vellora sends recorded audio to OpenAI for transcription. When cleanup is enabled, the transcript is also sent to OpenAI. Anyone with access to your Windows account can read your local history.</p>
      </section>
      </div>
      )}
    </div>
  );
}
