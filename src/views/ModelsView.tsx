import { Check, Gauge, Mic, Users } from "lucide-react";
import { useState } from "react";
import { BRAND } from "../lib/brand";
import type { AppSettings, SettingsChange, SttModel } from "../lib/types";
import { SectionTabs } from "./SectionTabs";
import { TextModelsView } from "./TextModelsView";

type Props = {
  settings: AppSettings;
  onSettingsChange: SettingsChange;
};

export const models: Array<{
  id: SttModel;
  name: string;
  bestFor: string;
  note: string;
  icon: React.ReactNode;
}> = [
  {
    id: "whisper-1",
    name: "Whisper 1",
    bestFor: "Legacy baseline",
    note: "The original transcription model. Try newer models to compare accuracy on your recordings.",
    icon: <Mic size={18} />,
  },
  {
    id: "gpt-4o-mini-transcribe",
    name: "GPT-4o mini Transcribe",
    bestFor: "Lower cost GPT transcription",
    note: "Good test candidate when you want newer transcription quality.",
    icon: <Gauge size={18} />,
  },
  {
    id: "gpt-4o-transcribe",
    name: "GPT-4o Transcribe",
    bestFor: "Higher accuracy",
    note: "Use for harder audio, names, or technical phrasing.",
    icon: <Check size={18} />,
  },
  {
    id: "gpt-4o-transcribe-diarize",
    name: "GPT-4o Transcribe Diarize",
    bestFor: "Conversations",
    note: "Uses the diarization model; speaker labels are not displayed in Vellora yet.",
    icon: <Users size={18} />,
  },
];

export type ModelsTab = "transcription" | "text";

const modelsTabs: ReadonlyArray<{ id: ModelsTab; label: string }> = [
  { id: "transcription", label: "Transcription" },
  { id: "text", label: "Text models" },
];

/** `initialTab` picks the tab shown when the view mounts, e.g. "text" when Settings links to Text Assist models. */
export function ModelsView({ settings, onSettingsChange, initialTab = "transcription" }: Props & { initialTab?: ModelsTab }) {
  const [tab, setTab] = useState<ModelsTab>(initialTab);
  return (
    <div className="models-page">
      <SectionTabs idPrefix="models" label="Model sections" tabs={modelsTabs} selected={tab} onSelect={setTab} />
      {tab === "transcription" ? (
        <div className="models-page" role="tabpanel" id="models-panel-transcription" aria-labelledby="models-tab-transcription">
          <TranscriptionModels settings={settings} onSettingsChange={onSettingsChange} />
        </div>
      ) : (
        <div role="tabpanel" id="models-panel-text" aria-labelledby="models-tab-text">
          <TextModelsView settings={settings} onSettingsChange={onSettingsChange} />
        </div>
      )}
    </div>
  );
}

function TranscriptionModels({ settings, onSettingsChange }: Props) {
  const [saving, setSaving] = useState(false);
  const [feedback, setFeedback] = useState<{ text: string; error: boolean } | null>(null);
  async function selectModel(model: SttModel) {
    if (saving || model === settings.sttModel) return;
    setSaving(true);
    setFeedback(null);
    try {
      await onSettingsChange({ sttModel: model });
      setFeedback({ text: "Transcription model saved.", error: false });
    } catch (error) { setFeedback({ text: `Could not save model: ${String(error)}`, error: true }); }
    finally { setSaving(false); }
  }

  return (
    <>
      <section className="panel models-intro">
        <h3>Choose how {BRAND.appName} listens</h3>
        <p>Compare models on your own recordings. GPT-4o mini Transcribe is the lower cost GPT option.</p>
        {feedback && <p className={feedback.error ? "error-text" : "success-text"} role={feedback.error ? "alert" : "status"}>{feedback.text}</p>}
        {saving && <p role="status">Saving model...</p>}
      </section>
      <section className="models-grid">
        {models.map((model) => {
          const active = settings.sttModel === model.id;
          return (
            <button className={`model-card ${active ? "active" : ""}`} aria-pressed={active} disabled={saving} key={model.id} onClick={() => void selectModel(model.id)}>
              <div className="model-icon">{model.icon}</div>
              <div>
                <h3>{model.name}</h3>
                <p>{model.note}</p>
                <span>{model.bestFor}</span>
              </div>
              {active && <Check className="model-check" size={18} />}
            </button>
          );
        })}
      </section>
    </>
  );
}
