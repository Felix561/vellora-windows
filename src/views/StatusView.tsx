import { AlertCircle, CheckCircle2, Loader2, Mic, Sparkles } from "lucide-react";
import type { AppSettings, AppSnapshot, TranscriptHistoryItem } from "../lib/types";

type Props = {
  settings: AppSettings;
  hasKey: boolean;
  snapshot: AppSnapshot;
  history: TranscriptHistoryItem[];
};

export function StatusView({ settings, hasKey, snapshot, history }: Props) {
  const latest = snapshot.lastTranscript ?? history[0];
  const active = ["recording", "transcribing", "cleaning", "pasting"].includes(snapshot.state);

  return (
    <div className="view-grid">
      <section className="panel hero-panel">
        <div className={`record-orb ${active ? "active" : ""}`}>
          {snapshot.state === "recording" ? <Mic size={42} /> : active ? <Loader2 size={42} /> : <CheckCircle2 size={42} />}
        </div>
        <div>
          <p className="eyebrow">Hold Ctrl + Win</p>
          <h2>{snapshot.state === "idle" ? "Ready for dictation" : stateCopy(snapshot.state)}</h2>
          <p className="muted">
            {snapshot.message ??
              (hasKey
                ? "Release the hotkey to transcribe, copy, and paste into the focused app."
                : "Add your OpenAI API key in settings before recording.")}
          </p>
        </div>
      </section>

      <section className="panel">
        <h3>Pipeline</h3>
        <div className="kv">
          <span>Provider</span>
          <strong>OpenAI</strong>
          <span>Transcription</span>
          <strong>{settings.sttModel}</strong>
          <span>Cleanup</span>
          <strong>{settings.cleanupEnabled ? settings.cleanupModel : "Off"}</strong>
          <span>Output</span>
          <strong>Paste + clipboard</strong>
        </div>
      </section>

      <section className="panel wide">
        <div className="section-title">
          <h3>Latest transcript</h3>
          {settings.cleanupEnabled && <Sparkles size={17} />}
        </div>
        <p className="transcript-preview">{latest?.finalText ?? "No transcripts yet."}</p>
      </section>

      {snapshot.state === "error" && (
        <section className="panel error-panel wide">
          <AlertCircle size={18} />
          <p>{snapshot.message ?? "Something went wrong."}</p>
        </section>
      )}
    </div>
  );
}

function stateCopy(state: AppSnapshot["state"]) {
  switch (state) {
    case "recording":
      return "Recording...";
    case "transcribing":
      return "Transcribing...";
    case "cleaning":
      return "Cleaning transcript...";
    case "pasting":
      return "Pasting...";
    case "done":
      return "Transcript pasted";
    case "error":
      return "Action needed";
    default:
      return "Ready for dictation";
  }
}
