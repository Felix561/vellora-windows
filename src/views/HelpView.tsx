import { useEffect, useState } from "react";
import { BookOpen, Copy, ExternalLink, LifeBuoy } from "lucide-react";
import { getSupportInfo, openHelpLink } from "../lib/api";
import { formatSupportInfo } from "../lib/help";
import type { AppSettings, HelpLinkId, SupportInfo } from "../lib/types";

type Props = { settings: AppSettings; onOpenSetup: () => void };

export function HelpView({ settings, onOpenSetup }: Props) {
  const [info, setInfo] = useState<SupportInfo | null>(null);
  const [infoError, setInfoError] = useState(false);
  const [copying, setCopying] = useState(false);
  const [feedback, setFeedback] = useState<{ text: string; error: boolean } | null>(null);
  const [diagnostics, setDiagnostics] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    getSupportInfo().then((value) => { if (active) setInfo(value); })
      .catch(() => { if (active) setInfoError(true); });
    return () => { active = false; };
  }, []);

  async function openLink(id: HelpLinkId) {
    setFeedback(null);
    try { await openHelpLink(id); }
    catch { setFeedback({ text: id === "windowsMicrophone" ? "Could not open Windows microphone settings. Open Settings → Privacy & security → Microphone manually." : "Could not open your browser. Try again, or visit github.com/Felix561/vellora-windows manually.", error: true }); }
  }

  async function copyDiagnostics() {
    if (copying) return;
    setCopying(true);
    setFeedback(null);
    try {
      const latest = await getSupportInfo();
      setInfo(latest);
      setInfoError(false);
      const text = formatSupportInfo(latest, settings);
      setDiagnostics(text);
      try {
        await navigator.clipboard.writeText(text);
        setFeedback({ text: "Support information copied. This replaces the current clipboard contents. Review it below before sharing.", error: false });
      } catch {
        setFeedback({ text: "Clipboard access is unavailable. Select and copy the support information below instead.", error: true });
      }
    } catch {
      setFeedback({ text: "Could not read support information. Please try again.", error: true });
    } finally { setCopying(false); }
  }

  const link = (id: HelpLinkId, label: string) => (
    <button className="icon-button" type="button" onClick={() => void openLink(id)}>
      <ExternalLink size={14} aria-hidden="true" />{label}
    </button>
  );

  return (
    <div className="help-stack">
      <section className="panel help-about">
        <div className="section-title"><div><p className="eyebrow">Your Windows dictation companion</p><h2>About Vellora</h2></div><LifeBuoy size={24} aria-hidden="true" /></div>
        <p>Hold Ctrl + Win, speak, and turn your voice into text in the app you are using.</p>
        <dl className="help-facts">
          <div><dt>Version</dt><dd>{info?.version ?? (infoError ? "Unavailable" : "Loading…")}</dd></div>
          <div><dt>Platform</dt><dd>Windows</dd></div>
          <div><dt>License</dt><dd>MIT · Copyright 2026 Felix Seitzer</dd></div>
        </dl>
        <div className="help-actions">{link("repository", "GitHub repository")}{link("license", "MIT license")}{link("readme", "User guide")}</div>
        <p className="hint">Source code and guides are available at Felix561/vellora-windows.</p>
      </section>

      <div className="help-grid">
        <section className="panel">
          <div className="section-title"><div><h3>Getting started</h3><p>A short guided setup you can reopen anytime.</p></div><BookOpen size={19} aria-hidden="true" /></div>
          <ol className="help-list">
            <li>Save an OpenAI API key. API usage has separate billing from ChatGPT.</li>
            <li>Choose your default microphone in Windows and allow desktop apps to use it.</li>
            <li>Place the cursor in a text field, hold Ctrl + Win, speak, then release.</li>
          </ol>
          <button className="icon-button" type="button" onClick={onOpenSetup}>Open guided setup</button>
          <p className="hint">Quickly tap Ctrl + Win twice, releasing both keys between taps, to record hands-free. Press Ctrl + Win again to stop. Recordings stop automatically after five minutes.</p>
        </section>

        <section className="panel">
          <h3>Privacy at a glance</h3>
          <p>Dictation sends recorded audio to OpenAI. Optional cleanup also sends the transcript. Vellora currently requires an internet connection for transcription.</p>
          <p>Your key is stored in Windows Credential Manager. Temporary audio is deleted after processing. If enabled, history stores the original and final text locally, unencrypted.</p>
          <p className="hint">Turning off history affects future dictations. Delete existing entries in History. The latest transcript remains available for manual copy until the next dictation or quitting.</p>
          <div className="help-actions">{link("privacy", "Privacy details")}</div>
        </section>
      </div>

      <section className="panel">
        <div className="section-title"><div><h3>Common problems</h3><p>Start with these checks before reporting a bug.</p></div></div>
        <div className="help-questions">
          <details><summary>No recording or no microphone input</summary><p>Select the right input in Windows Sound settings. In Settings → Privacy & security → Microphone, enable microphone access and access for desktop apps. Check whether another app or your security software blocks the device.</p><div className="help-actions">{link("windowsMicrophone", "Windows microphone settings")}</div></details>
          <details><summary>Transcription fails or an API error appears</summary><p>Check your internet connection, API key and OpenAI API billing. A ChatGPT subscription does not include API usage. Vellora preserves available transcript text when cleanup or pasting fails; look on the Dashboard before retrying.</p><div className="help-actions">{link("openaiKeys", "OpenAI API keys")}{link("openaiBilling", "OpenAI API billing")}</div></details>
          <details><summary>The transcript did not paste</summary><p>Keep the destination app and text field focused until processing ends. Switching browser tabs or fields may change where text is pasted. Use Copy on the Dashboard, or Paste and its three-second countdown to focus the destination. Pasting into an app running as administrator can be blocked by Windows.</p><p>When clipboard saving is off, Vellora briefly uses the clipboard and restores previous plain text. Images, files and rich formatting cannot always be preserved; automatic paste is skipped for a clipboard containing only non-text data.</p></details>
          <details><summary>The shortcut or startup setting does not work</summary><p>Check the Dashboard's System status, quit Vellora from the tray, and start it again. Another shortcut tool can intercept Ctrl + Win. Enable or disable sign-in startup under Settings → General → Windows startup; it starts quietly in the tray.</p></details>
        </div>
        <div className="help-actions">{link("troubleshooting", "Full troubleshooting guide")}</div>
      </section>

      <section className="panel">
        <div className="section-title"><div><h3>Safe support information</h3><p>Copy basic version, status and configuration details to help investigate a problem.</p></div></div>
        <p className="hint">Excludes API keys, transcript text, recordings, device names, file paths, usernames and error messages. Nothing is sent automatically. Copying replaces your clipboard.</p>
        <div className="help-actions"><button className="icon-button" type="button" disabled={copying} onClick={() => void copyDiagnostics()}><Copy size={15} aria-hidden="true" />{copying ? "Reading support information…" : "Copy support information"}</button></div>
        {feedback && <p className={feedback.error ? "error-text" : "success-text"} role={feedback.error ? "alert" : "status"}>{feedback.text}</p>}
        {diagnostics && <textarea className="support-preview" rows={12} readOnly aria-label="Support information to review before sharing" value={diagnostics} />}
      </section>
    </div>
  );
}
