import { useEffect, useMemo, useState } from "react";
import { AlertCircle, Check, Clipboard, Loader2, Mic, Sparkles } from "lucide-react";
import { getDictationSnapshot, getSettings, onSettingsChanged, onAudioLevel, onOverlayStateChanged } from "../lib/api";
import { useAppearance } from "../lib/appearance";
import type { Appearance, DictationState } from "../lib/types";

const bars = [0.35, 0.7, 1, 0.75, 0.45, 0.85, 0.55];

export function OverlayView() {
  const [appearance, setAppearance] = useState<Appearance>("notebook");
  useAppearance(appearance);
  const [state, setState] = useState<DictationState>("idle");
  const [level, setLevel] = useState(0);
  const [message, setMessage] = useState<string | null>("Ready");

  useEffect(() => {
    getSettings().then((settings) => setAppearance(settings.appearance)).catch(console.error);
    getDictationSnapshot()
      .then((snapshot) => {
        setState(snapshot.state);
        setMessage(snapshot.message ?? "Ready");
      })
      .catch(console.error);
    const unlisteners = [
      onSettingsChanged((settings) => setAppearance(settings.appearance)),
      onOverlayStateChanged((event) => {
        setState(event.state);
        setMessage(event.message ?? null);
        const nextLevel = event.level;
        if (typeof nextLevel === "number") {
          setLevel((current) => current * 0.55 + nextLevel * 0.45);
        }
      }),
      onAudioLevel((event) => setLevel((current) => current * 0.55 + event.level * 0.45)),
    ];
    return () => {
      unlisteners.forEach((promise) => promise.then((unlisten) => unlisten()).catch(console.error));
    };
  }, []);

  const mode = state === "done" ? "success" : state;
  const label = useMemo(() => {
    if (mode === "idle") return "";
    if (mode === "recording") return message === "Locked recording" ? "Locked recording" : "Recording";
    if (mode === "transcribing") return "Transcribing";
    if (mode === "cleaning") return "Cleaning";
    if (mode === "pasting") return "Pasting";
    if (mode === "success") return message ?? "Transcript ready";
    if (mode === "error") return message ?? "Error";
    return message ?? "";
  }, [message, mode]);

  return (
    <main className={`overlay-root overlay-${mode}`}>
      <section className="overlay-pill">
        {mode === "idle" && <span className="idle-light" />}
        {mode === "recording" && (
          <>
            <Mic size={16} />
            <Waveform level={level} />
            <strong>{label}</strong>
          </>
        )}
        {(mode === "transcribing" || mode === "cleaning") && (
          <>
            {mode === "cleaning" ? <Sparkles size={16} /> : <Loader2 className="spin" size={16} />}
            <Waveform level={0.5} animated />
            <strong>{label}</strong>
          </>
        )}
        {mode === "pasting" && (
          <>
            <Clipboard size={16} />
            <strong>{label}</strong>
          </>
        )}
        {mode === "success" && (
          <>
            <Check size={16} />
            <strong>{label}</strong>
          </>
        )}
        {mode === "error" && (
          <>
            <AlertCircle size={16} />
            <strong>{label}</strong>
          </>
        )}
      </section>
    </main>
  );
}

function Waveform({ level, animated = false }: { level: number; animated?: boolean }) {
  const visibleLevel = Math.max(0.18, level);
  return (
    <div className={`waveform ${animated ? "animated" : ""}`}>
      {bars.map((factor, index) => (
        <span key={index} style={{ height: `${8 + visibleLevel * 34 * factor}px` }} />
      ))}
    </div>
  );
}
