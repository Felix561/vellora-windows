import { useEffect, useRef, useState } from "react";
import { CheckCircle2, ExternalLink, KeyRound, Mic } from "lucide-react";
import { checkMicrophone, openHelpLink, saveOpenAiApiKey } from "../lib/api";
import type { AppSnapshot, HelpLinkId, MicrophoneCheck } from "../lib/types";

type Props = {
  hasKey: boolean;
  snapshot: AppSnapshot;
  onKeyStatusChange: (hasKey: boolean) => void;
  onFinish: (choice: "complete" | "skipped") => void;
};

const steps = ["OpenAI key", "Microphone", "Shortcut", "Try dictation"];

export function OnboardingView({ hasKey, snapshot, onKeyStatusChange, onFinish }: Props) {
  const [step, setStep] = useState(0);
  const [draftKey, setDraftKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<{ text: string; error: boolean } | null>(null);
  const [microphone, setMicrophone] = useState<MicrophoneCheck | null>(null);
  const [practiceComplete, setPracticeComplete] = useState(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const continueButton = useRef<HTMLButtonElement>(null);
  const keyWasConfigured = useRef(hasKey);
  const practiceBaseline = useRef<string | undefined>();

  useEffect(() => {
    heading.current?.focus();
    setFeedback(null);
    if (step === 3) {
      practiceBaseline.current = snapshot.lastTranscript?.id;
      setPracticeComplete(false);
    }
    // A previous transcript is not evidence that this optional practice succeeded.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step]);

  useEffect(() => {
    const transcript = snapshot.lastTranscript;
    if (step === 3 && transcript && transcript.id !== practiceBaseline.current && !transcript.error && transcript.finalText.trim()) {
      setPracticeComplete(true);
    }
  }, [step, snapshot.lastTranscript]);

  useEffect(() => {
    // Saving removes the focused password form. Give keyboard users a useful
    // next position without moving focus when reopening setup with a saved key.
    if (step === 0 && hasKey && !keyWasConfigured.current) continueButton.current?.focus();
    keyWasConfigured.current = hasKey;
  }, [hasKey, step]);

  async function saveKey() {
    if (!draftKey.trim() || busy) return;
    setBusy(true);
    setFeedback(null);
    try {
      await saveOpenAiApiKey(draftKey.trim());
      setDraftKey("");
      onKeyStatusChange(true);
      setFeedback({ text: "API key saved in Windows Credential Manager. It has not been tested with an API request.", error: false });
    } catch {
      setFeedback({ text: "Could not save the API key. Your setup has not advanced. Please try again.", error: true });
    } finally { setBusy(false); }
  }

  async function testMicrophone() {
    if (busy) return;
    setBusy(true);
    setFeedback(null);
    setMicrophone(null);
    try {
      const result = await checkMicrophone();
      setMicrophone(result);
      if (!result.available) setFeedback({ text: "The default microphone could not be confirmed. Connect or enable an input device in Windows, check its settings, then try again.", error: true });
    } catch {
      setFeedback({ text: "Could not check the default microphone. Check Windows input and microphone privacy settings, then try again.", error: true });
    } finally { setBusy(false); }
  }

  async function openLink(id: HelpLinkId) {
    try { await openHelpLink(id); }
    catch { setFeedback({ text: id === "windowsMicrophone" ? "Could not open Windows microphone settings. Open Settings → Privacy & security → Microphone manually." : "Could not open your browser. Open the OpenAI dashboard manually.", error: true }); }
  }

  return (
    <section className="panel onboarding-panel" aria-label="Guided setup">
      <p className="eyebrow">A few minutes to get started</p>
      <h2 ref={heading} tabIndex={-1}>Set up Vellora · {steps[step]}</h2>
      <ol className="setup-steps" aria-label="Setup progress">{steps.map((name, index) => <li key={name} aria-current={step === index ? "step" : undefined}><span aria-hidden="true">{index + 1}</span>{name}</li>)}</ol>

      {step === 0 && <div className="setup-content">
        <h3>{hasKey ? "Your API key is already saved" : "Connect your OpenAI API key"}</h3>
        <p>Vellora uses OpenAI to transcribe your voice. Audio is sent to OpenAI when you finish a recording; optional cleanup also sends transcript text. API usage is billed separately from a ChatGPT subscription.</p>
        {hasKey ? <p className="setup-check"><CheckCircle2 size={18} aria-hidden="true" />A key is saved in Windows Credential Manager. You can replace or remove it in Settings.</p> : <form className="input-row setup-key" onSubmit={(event) => { event.preventDefault(); void saveKey(); }}>
          <KeyRound size={17} aria-hidden="true" /><input type="password" aria-label="OpenAI API key" autoComplete="off" spellCheck={false} value={draftKey} onChange={(event) => setDraftKey(event.target.value)} disabled={busy} placeholder="Paste OpenAI API key" />
          <button type="submit" disabled={busy || !draftKey.trim()}>{busy ? "Saving…" : "Save API key"}</button>
        </form>}
        <div className="help-actions"><button className="icon-button" type="button" onClick={() => void openLink("openaiKeys")}><ExternalLink size={14} aria-hidden="true" />OpenAI API keys</button><button className="icon-button" type="button" onClick={() => void openLink("openaiBilling")}>API billing</button></div>
        <p className="hint">Saving the key makes no paid request. Transcript history is stored locally as unencrypted text when enabled; change history and clipboard options in Settings.</p>
      </div>}

      {step === 1 && <div className="setup-content">
        <h3>Check your Windows microphone</h3>
        <p>Vellora uses the default Windows input device. Choose it under Windows Settings → System → Sound → Input, then enable microphone access for desktop apps under Privacy & security → Microphone.</p>
        <div className="help-actions"><button className="icon-button" type="button" disabled={busy} onClick={() => void testMicrophone()}><Mic size={16} aria-hidden="true" />{busy ? "Checking…" : "Check microphone"}</button><button className="icon-button" type="button" onClick={() => void openLink("windowsMicrophone")}>Windows microphone settings</button></div>
        {microphone?.available && <p className="setup-check" role="status"><CheckCircle2 size={18} aria-hidden="true" />Default microphone found{microphone.deviceName ? `: ${microphone.deviceName}` : "."}</p>}
        <p className="hint">This local check looks for an input device. It does not record, confirm sound levels or upload audio, and makes no paid request. Use the Windows input meter to check that your voice reaches the microphone.</p>
      </div>}

      {step === 2 && <div className="setup-content">
        <h3>One shortcut, two ways to record</h3>
        <dl className="shortcut-guide"><div><dt>Hold to dictate</dt><dd>Hold <kbd>Ctrl</kbd> + <kbd>Win</kbd>, speak, then release to transcribe.</dd></div><div><dt>Hands-free recording</dt><dd>Quickly tap Ctrl + Win twice, releasing both keys between taps. Press Ctrl + Win again when you are finished.</dd></div></dl>
        <p>The top-center indicator shows recording and processing without blocking clicks. Recordings stop automatically after five minutes.</p>
        <p className="hint">Start with the cursor in the destination text field and keep it there until processing finishes. If automatic pasting fails, the Dashboard keeps your latest transcript for manual copy.</p>
      </div>}

      {step === 3 && <div className="setup-content">
        <h3>Optional: try your first dictation</h3>
        <ol className="help-list"><li>Open Notepad and click in the empty document.</li><li>Hold Ctrl + Win and say a short sentence.</li><li>Release the keys and wait for processing. If automatic paste is enabled, check the text in Notepad; otherwise copy it from the Dashboard.</li></ol>
        <p className="hint">This optional practice sends your recording to OpenAI and uses your API billing. Setup itself does not start a recording or request transcription.</p>
        {practiceComplete ? <p className="setup-check" role="status"><CheckCircle2 size={18} aria-hidden="true" />A new transcript was created. Check its wording and paste destination yourself.</p> : <p className="setup-state" role="status">{snapshot.state === "recording" ? "Recording in progress…" : ["transcribing", "cleaning", "pasting"].includes(snapshot.state) ? "Processing your recording…" : "Practice is optional. You can finish setup without recording."}</p>}
        <p>Return to Settings to choose Notebook or Windows Classic, manage startup, and change clipboard or history preferences. Help & About keeps these instructions available.</p>
      </div>}

      {feedback && <p className={feedback.error ? "error-text" : "success-text"} role={feedback.error ? "alert" : "status"}>{feedback.text}</p>}
      <div className="setup-footer">
        <button className="icon-button" type="button" disabled={busy} onClick={() => onFinish("skipped")}>Skip for now</button>
        <div>{step > 0 && <button className="icon-button" type="button" disabled={busy} onClick={() => setStep(step - 1)}>Back</button>}{step < 3 ? <button ref={continueButton} className="icon-button" type="button" disabled={busy || (step === 0 && !hasKey)} onClick={() => setStep(step + 1)}>Continue</button> : <button className="icon-button" type="button" disabled={busy} onClick={() => onFinish("complete")}>Finish setup</button>}</div>
      </div>
    </section>
  );
}
