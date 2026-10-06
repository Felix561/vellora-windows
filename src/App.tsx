import { useCallback, useEffect, useRef, useState } from "react";
import { BarChart3, Clipboard, History, KeyRound, LifeBuoy, Settings, SlidersHorizontal } from "lucide-react";
import {
  getDictationSnapshot,
  getAppHealth,
  getSettings,
  hasOpenAiApiKey,
  listTranscripts,
  onAppHealthChanged,
  onSettingsChanged,
  onDiagnosticEvent,
  onDictationError,
  onDictationStateChanged,
  onOpenDashboardRequested,
  onTranscriptCreated,
  onHistoryChanged,
  onOverlayStateChanged,
  saveSettings,
} from "./lib/api";
import { useAppearance } from "./lib/appearance";
import { BRAND } from "./lib/brand";
import { hasDismissedSetup, rememberSetup } from "./lib/help";
import type { AppHealthSnapshot, AppSettings, AppSnapshot, DiagnosticEvent, TranscriptHistoryItem } from "./lib/types";
import velloraSymbol from "./assets/brand/vellora-symbol.png";
import { DashboardView } from "./views/DashboardView";
import { HistoryView } from "./views/HistoryView";
import { ModelsView } from "./views/ModelsView";
import { SettingsView } from "./views/SettingsView";
import { StartupView } from "./views/StartupView";
import { HelpView } from "./views/HelpView";
import { OnboardingView } from "./views/OnboardingView";

type Tab = "dashboard" | "history" | "models" | "settings" | "help" | "setup";

export default function App() {
  const [tab, setTab] = useState<Tab>("dashboard");
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [hasKey, setHasKey] = useState(false);
  const [keyStatusKnown, setKeyStatusKnown] = useState(false);
  const [setupNotice, setSetupNotice] = useState<string | null>(null);
  const setupConsidered = useRef(false);
  const [snapshot, setSnapshot] = useState<AppSnapshot>({ state: "idle" });
  const [history, setHistory] = useState<TranscriptHistoryItem[]>([]);
  const [health, setHealth] = useState<AppHealthSnapshot | null>(null);
  const [diagnostics, setDiagnostics] = useState<DiagnosticEvent[]>([]);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [backendError, setBackendError] = useState<string | null>(null);
  const [dictationError, setDictationError] = useState<string | null>(null);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const mounted = useRef(false);
  const refreshId = useRef(0);
  const revisions = useRef({ settings: 0, key: 0, snapshot: 0, health: 0 });
  const settingsRef = useRef<AppSettings | null>(null);
  const snapshotRef = useRef<AppSnapshot>({ state: "idle" });
  const settingsWrites = useRef<Promise<void>>(Promise.resolve());

  const applySettings = useCallback((next: AppSettings) => {
    revisions.current.settings += 1;
    settingsRef.current = next;
    setSettings(next);
    setLoadError(null);
  }, []);

  const applySnapshot = useCallback((next: AppSnapshot) => {
    revisions.current.snapshot += 1;
    snapshotRef.current = next;
    setSnapshot(next);
  }, []);

  const updateKeyStatus = useCallback((present: boolean) => {
    revisions.current.key += 1;
    setHasKey(present);
    setKeyStatusKnown(true);
  }, []);

  const updateSettings = useCallback((patch: Partial<AppSettings>): Promise<void> => {
    // Navigation can unmount a view while its write is pending. Queue patches
    // centrally, then merge when they run, never into that view's old props.
    const write = settingsWrites.current.then(async () => {
      if (!mounted.current || !settingsRef.current) throw new Error("Settings are unavailable. Please try again.");
      revisions.current.settings += 1;
      setSettingsError(null);
      try {
        const saved = await saveSettings({ ...settingsRef.current, ...patch });
        if (mounted.current) applySettings(saved);
      } catch (error) {
        if (mounted.current) setSettingsError(`Could not save settings: ${String(error)}`);
        throw error;
      }
    });
    // A rejected write reports to its caller without poisoning later edits.
    settingsWrites.current = write.catch(() => undefined);
    return write;
  }, [applySettings]);

  useAppearance(settings?.appearance);

  const refresh = useCallback(async () => {
    const requestId = ++refreshId.current;
    const before = { ...revisions.current };
    const [nextSettings, keyPresent, nextSnapshot, transcripts, nextHealth] = await Promise.allSettled([
      getSettings(),
      hasOpenAiApiKey(),
      getDictationSnapshot(),
      listTranscripts(100),
      getAppHealth(),
    ]);
    // A deletion/new transcript starts another refresh. Events and acknowledged
    // edits invalidate only their resource, so slow reads cannot resurrect data.
    if (!mounted.current || requestId !== refreshId.current) return;
    const current = {
      settings: before.settings === revisions.current.settings,
      key: before.key === revisions.current.key,
      snapshot: before.snapshot === revisions.current.snapshot,
      health: before.health === revisions.current.health,
    };
    if (current.settings) {
      if (nextSettings.status === "fulfilled") applySettings(nextSettings.value);
      else setLoadError(String(nextSettings.reason));
    }
    if (current.key) {
      if (keyPresent.status === "fulfilled") updateKeyStatus(keyPresent.value);
      else { revisions.current.key += 1; setKeyStatusKnown(false); }
    }
    if (current.snapshot && nextSnapshot.status === "fulfilled") {
      applySnapshot(nextSnapshot.value);
      if (nextSnapshot.value.state === "error") setDictationError(nextSnapshot.value.message ?? "Dictation failed. Please try again.");
    }
    if (transcripts.status === "fulfilled") {
      setHistory(transcripts.value);
      setHistoryError(null);
    } else {
      setHistoryError(`Could not load local history: ${String(transcripts.reason)}`);
    }
    if (current.health && nextHealth.status === "fulfilled") {
      revisions.current.health += 1;
      setHealth(nextHealth.value);
    }
    const unavailable = [
      current.key && keyPresent.status === "rejected" ? "API key status" : null,
      current.snapshot && nextSnapshot.status === "rejected" ? "dictation status" : null,
      current.health && nextHealth.status === "rejected" ? "backend health" : null,
    ].filter(Boolean);
    setBackendError(unavailable.length ? `Could not read ${unavailable.join(", ")}. The displayed status may be out of date.` : null);
  }, [applySettings, applySnapshot, updateKeyStatus]);

  useEffect(() => {
    mounted.current = true;
    let active = true;
    refresh().catch(console.error);
    const unlisteners = [
      onDictationStateChanged((state) => {
        if (!active) return;
        applySnapshot({ ...snapshotRef.current, state, recordingMode: state === "recording" ? snapshotRef.current.recordingMode : null });
        if (state === "recording") setDictationError(null);
      }),
      onTranscriptCreated((item) => {
        if (!active) return;
        applySnapshot({ state: item.error ? "error" : "done", lastTranscript: item, message: item.error ?? (item.pasted ? "Transcript pasted" : "Transcript ready") });
        setDictationError(item.error ?? null);
        void refresh();
      }),
      onDictationError((message) => { if (active) { applySnapshot({ ...snapshotRef.current, state: "error", message, recordingMode: null }); setDictationError(message); } }),
      onDiagnosticEvent((event) => { if (active) setDiagnostics((items) => [event, ...items.filter((existing) => existing.id !== event.id)].slice(0, 5)); }),
      onAppHealthChanged((next) => { if (active) { revisions.current.health += 1; setHealth(next); } }),
      onSettingsChanged((next) => { if (active) applySettings(next); }),
      onOpenDashboardRequested(() => { if (active) setTab("dashboard"); }),
      onHistoryChanged(() => {
        if (!active) return;
        // This backend event means deletion/clear completed. Drop cached text
        // before slow queries or later state-only events can carry it forward.
        setHistory([]);
        applySnapshot({ ...snapshotRef.current, lastTranscript: null });
        void refresh();
      }),
      onOverlayStateChanged((event) => {
        // Audio-level events are frequent. Only a recording-mode change needs
        // a main-window update; no extra polling or waveform renders here.
        const current = snapshotRef.current;
        if (active && current.state === "recording" && event.state === "recording" && event.recordingMode && event.recordingMode !== current.recordingMode) {
          applySnapshot({ ...current, recordingMode: event.recordingMode });
        }
      }),
    ];
    return () => {
      active = false;
      mounted.current = false;
      refreshId.current += 1;
      unlisteners.forEach((promise) => promise.then((unlisten) => unlisten()).catch(console.error));
    };
  }, [refresh, applySettings, applySnapshot]);

  useEffect(() => {
    if (!settings || !keyStatusKnown || setupConsidered.current) return;
    setupConsidered.current = true;
    if (!hasKey && !hasDismissedSetup()) setTab("setup");
  }, [settings, keyStatusKnown, hasKey]);

  function finishSetup(choice: "complete" | "skipped") {
    setSetupNotice(rememberSetup(choice) ? null : "Setup was closed, but the reminder preference could not be saved. It may appear again next time.");
    setTab("dashboard");
  }

  if (!settings) {
    return <StartupView error={loadError} onRetry={refresh} />;
  }

  const title = tab === "dashboard" ? "Dashboard" : tab === "history" ? "History" : tab === "models" ? "AI Models" : tab === "settings" ? "Settings" : tab === "help" ? "Help & About" : "Guided setup";

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">
            <img src={velloraSymbol} alt="" />
          </div>
          <div>
            <strong>{BRAND.appName}</strong>
            <span>{BRAND.tagline}</span>
          </div>
        </div>

        <nav className="nav" aria-label="Main navigation">
          <button className={tab === "dashboard" ? "active" : ""} aria-current={tab === "dashboard" ? "page" : undefined} onClick={() => setTab("dashboard")}>
            <BarChart3 size={17} />
            Dashboard
          </button>
          <button className={tab === "history" ? "active" : ""} aria-current={tab === "history" ? "page" : undefined} onClick={() => setTab("history")}>
            <History size={17} />
            History
          </button>
          <button className={tab === "models" ? "active" : ""} aria-current={tab === "models" ? "page" : undefined} onClick={() => setTab("models")}>
            <SlidersHorizontal size={17} />
            AI Models
          </button>
          <button className={tab === "settings" ? "active" : ""} aria-current={tab === "settings" ? "page" : undefined} onClick={() => setTab("settings")}>
            <Settings size={17} />
            Settings
          </button>
          <button className={tab === "help" || tab === "setup" ? "active" : ""} aria-current={tab === "help" || tab === "setup" ? "page" : undefined} onClick={() => setTab("help")}>
            <LifeBuoy size={17} />
            Help & About
          </button>
        </nav>

        <div className="hotkey-box">
          <KeyRound size={16} />
          <div>
            <span>Hold to dictate</span>
            <strong>Ctrl + Win</strong>
          </div>
        </div>
      </aside>

      <section className="content">
        <header className="topbar">
          <div className="topbar-title">
            <p>{settings.sttModel}</p>
            <h1>{title}</h1>
          </div>
          <div className={hasKey ? "key-pill ok" : "key-pill"}>
            <Clipboard size={15} />
            {!keyStatusKnown ? "API key status unavailable" : hasKey ? "API key saved" : "API key missing"}
          </div>
        </header>

        {historyError && <p className="error-banner" role="alert">{historyError} <button className="icon-button" onClick={() => void refresh()}>Retry</button></p>}
        {setupNotice && <p className="error-banner" role="alert">{setupNotice} <button className="icon-button" onClick={() => setSetupNotice(null)}>Dismiss</button></p>}
        {settingsError && <p className="error-banner" role="alert">{settingsError} <button className="icon-button" onClick={() => setSettingsError(null)}>Dismiss</button></p>}
        {backendError && <p className="error-banner" role="alert">{backendError} <button className="icon-button" onClick={() => void refresh()}>Retry</button></p>}
        {dictationError && <div className="error-banner" role="alert"><strong>Dictation needs attention</strong><p>{dictationError}</p><div className="history-toolbar">{tab !== "dashboard" && <button className="icon-button" onClick={() => setTab("dashboard")}>Open Dashboard</button>}<button className="icon-button" onClick={() => setDictationError(null)}>Dismiss</button></div></div>}
        {tab === "dashboard" && (
          <DashboardView
            settings={settings}
            hasKey={hasKey}
            keyStatusKnown={keyStatusKnown}
            snapshot={snapshot}
            history={history}
            health={health}
            diagnostics={diagnostics}
          />
        )}
        {tab === "history" && <HistoryView items={history} onRefresh={refresh} />}
        {tab === "models" && <ModelsView settings={settings} onSettingsChange={updateSettings} />}
        {tab === "settings" && (
          <SettingsView
            settings={settings}
            hasKey={hasKey}
            onSettingsChange={updateSettings}
            onKeyStatusChange={updateKeyStatus}
          />
        )}
        {tab === "help" && <HelpView settings={settings} onOpenSetup={() => setTab("setup")} />}
        {tab === "setup" && <OnboardingView hasKey={hasKey} snapshot={snapshot} onKeyStatusChange={updateKeyStatus} onFinish={finishSetup} />}
      </section>
      {settings.appearance === "classic" && (
        <footer className="classic-statusbar">
          <span role="status">{snapshot.state === "idle" ? "Ready" : snapshot.state === "done" ? (snapshot.lastTranscript?.pasted ? "Transcript pasted" : "Transcript ready") : snapshot.state}</span>
          <span>Ctrl + Win to dictate</span>
          <span>Vellora</span>
        </footer>
      )}
    </main>
  );
}
