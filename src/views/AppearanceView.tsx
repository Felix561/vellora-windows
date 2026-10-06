import { useState } from "react";
import type { Appearance, AppSettings, SettingsChange } from "../lib/types";

type Props = { settings: AppSettings; onSettingsChange: SettingsChange };
const themes: Array<{ value: Appearance; title: string; description: string }> = [
  { value: "notebook", title: "Notebook", description: "Warm paper, soft colors, and quiet typography." },
  { value: "classic", title: "Windows Classic", description: "Windows 95/98 style: gray panels, blue title bars, and raised controls." },
];

export function AppearanceView({ settings, onSettingsChange }: Props) {
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function selectAppearance(appearance: Appearance) {
    if (appearance === settings.appearance || saving) return;
    setSaving(true);
    setError(null);
    try { await onSettingsChange({ appearance }); }
    catch (err) { setError(`Could not save appearance: ${String(err)}`); }
    finally { setSaving(false); }
  }
  return (
    <section className="panel appearance-panel">
      <div className="section-title"><div><h3>Appearance</h3><p>Choose the look of your writing companion.</p></div></div>
      <div className="appearance-options" role="radiogroup" aria-label="Application theme" aria-busy={saving}>
        {themes.map((theme) => (
          <label className={`appearance-option ${settings.appearance === theme.value ? "selected" : ""}`} key={theme.value}>
            <div className={`theme-preview preview-${theme.value}`} aria-hidden="true">
              <div className="preview-title">Vellora</div>
              <div className="preview-body"><div className="preview-sidebar"><i /><i /><i /></div>
                <div className="preview-content"><b>Ready for dictation</b><div className="preview-lines"><i /><i /></div><span>Ctrl + Win</span></div>
              </div>
            </div>
            <div className="appearance-label"><input type="radio" name="appearance" value={theme.value}
              checked={settings.appearance === theme.value} disabled={saving}
              onChange={() => selectAppearance(theme.value)} /><strong>{theme.title}</strong></div>
            <p>{theme.description}</p>
          </label>
        ))}
      </div>
      <p className="hint">Applies to every page and the status indicator. Your choice is saved automatically.</p>
      <p className="appearance-feedback" role="status">{saving ? "Saving appearance..." : ""}</p>
      {error && <p className="warning-text" role="alert">{error}</p>}
    </section>
  );
}
