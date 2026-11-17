import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import { Loader2, X } from "lucide-react";
import { useAppearance } from "../lib/appearance";
import {
  PANEL_OPEN_EVENT,
  PANEL_RESULT_EVENT,
  billingLabel,
  canApply,
  formatPanelCost,
  formatPanelDuration,
  friendlyWarnings,
  getPanelInit,
  isCustomPreset,
  nextModelIndex,
  normalizePanelInit,
  normalizePanelResult,
  panelApply,
  panelClose,
  panelCopy,
  panelRun,
  presetKeyRange,
  rememberInstruction,
  resultKey,
  safeColor,
  targetAppName,
  cardKeyRange,
  CUSTOM_INSTRUCTION_MAX_CHARS,
  type EnsemblePreview,
  type PanelInit,
  type PanelResult,
} from "../lib/assistPanel";
import { wordDiff, type DiffPart } from "../lib/wordDiff";
import { cardCount, finishedCount, useEnsemble, type PreviewState } from "../lib/assistEnsemble";
import { EnsembleCards, EnsemblePreviewView, ProfileChips, ResultText, WarningList } from "./AssistEnsembleView";
import "./assist-panel.css";

const NOTICE_MS = 2500;

type PanelStyle = CSSProperties & Record<`--${string}`, string>;

function errorMessage(error: unknown) {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "Something went wrong";
}

/**
 * Content of the `assist-panel` window (Rewrite panel, hold Shift+Win). The backend opens it
 * next to the selection, sends `text-assist-panel-open` with the session, and streams one
 * `text-assist-panel` event per update. Results are cached by the backend per preset + model;
 * the view shows whichever result matches the selected preset and model.
 */
export function AssistPanelView() {
  const [init, setInit] = useState<PanelInit | null>(null);
  const [presetId, setPresetId] = useState("");
  const [modelIndex, setModelIndex] = useState(0);
  const [results, setResults] = useState<Record<string, PanelResult>>({});
  const [showDiff, setShowDiff] = useState(false);
  const [draft, setDraft] = useState("");
  const [historyIndex, setHistoryIndex] = useState(-1);
  const [history, setHistory] = useState<string[]>([]);
  const [notice, setNotice] = useState<{ id: number; text: string } | null>(null);
  const [applying, setApplying] = useState(false);
  const [focusInputToken, setFocusInputToken] = useState(0);
  /** Custom instruction last run in this session (reused when the model changes). */
  const [lastCustom, setLastCustom] = useState<string | null>(null);
  useAppearance(init?.appearance ?? "notebook");

  const sessionRef = useRef<string | null>(null);
  /** Request id that `panel_run` returned last for each preset|model key. */
  const latestRequestRef = useRef(new Map<string, string>());
  /** Request ids replaced by a newer run of the same key; their events are ignored. */
  const staleRef = useRef(new Set<string>());
  /** Custom instruction last sent for each custom|model key. */
  const customUsedRef = useRef(new Map<string, string>());
  /** Custom instruction last run in this session (reused when the model changes). */
  const lastCustomRef = useRef<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const noticeId = useRef(0);
  const ensemble = useEnsemble(sessionRef);
  const { reset: resetEnsemble, handleResult: handleEnsembleResult } = ensemble;

  const showNotice = useCallback((text: string) => {
    noticeId.current += 1;
    setNotice({ id: noticeId.current, text });
  }, []);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice((current) => (current?.id === notice.id ? null : current)), NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const startRun = useCallback((runPresetId: string, runModelIndex: number, requestedFresh: boolean, instruction?: string) => {
    const session = sessionRef.current;
    if (!session || !runPresetId) return;
    const key = resultKey(runPresetId, runModelIndex);
    let fresh = requestedFresh;
    let customInstruction: string | null = null;
    if (isCustomPreset(runPresetId)) {
      customInstruction = (instruction ?? lastCustomRef.current ?? "").trim() || null;
      if (!customInstruction) return;
      // The backend caches per preset + model; a different instruction needs a new request.
      if (customUsedRef.current.get(key) !== customInstruction) fresh = true;
      customUsedRef.current.set(key, customInstruction);
      lastCustomRef.current = customInstruction;
      setLastCustom(customInstruction);
    }
    const previous = latestRequestRef.current.get(key);
    if (fresh && previous) staleRef.current.add(previous);
    setResults((current) => {
      const existing = current[key];
      // Idempotent, so safe in an updater that StrictMode may call twice.
      if (fresh && existing?.requestId) staleRef.current.add(existing.requestId);
      if (!existing || (!fresh && existing.phase === "done")) return current;
      const next = { ...current };
      delete next[key];
      return next;
    });
    panelRun({ presetId: runPresetId, modelIndex: runModelIndex, customInstruction, fresh })
      .then((requestId) => {
        // A replay (`fresh: false`) may reuse the cached request id, so only `fresh` runs
        // retire the previous id (above, before the new run starts).
        if (sessionRef.current === session) latestRequestRef.current.set(key, requestId);
      })
      .catch((error: unknown) => {
        if (sessionRef.current !== session) return;
        setResults((current) => ({
          ...current,
          [key]: {
            sessionId: session,
            requestId: "",
            presetId: runPresetId,
            modelIndex: runModelIndex,
            phase: "failed",
            text: "",
            label: "",
            effort: null,
            billing: "",
            color: "",
            durationMs: null,
            costUsd: null,
            warnings: [],
            error: errorMessage(error),
          },
        }));
      });
  }, []);

  const applyInit = useCallback((raw: unknown) => {
    const next = normalizePanelInit(raw);
    if (!next || next.sessionId === sessionRef.current) return;
    sessionRef.current = next.sessionId;
    latestRequestRef.current = new Map();
    staleRef.current = new Set();
    customUsedRef.current = new Map();
    lastCustomRef.current = null;
    setLastCustom(null);
    const startsCustom = isCustomPreset(next.activePresetId);
    const firstInstruction = startsCustom ? next.customInstructions[0] ?? "" : "";
    setInit(next);
    setPresetId(next.activePresetId);
    setModelIndex(next.activeModelIndex);
    setResults({});
    setShowDiff(false);
    setDraft(firstInstruction);
    setHistoryIndex(firstInstruction ? 0 : -1);
    setHistory(next.customInstructions);
    setNotice(null);
    setApplying(false);
    resetEnsemble();
    if (startsCustom && !firstInstruction) {
      setFocusInputToken((token) => token + 1);
      return;
    }
    startRun(next.activePresetId, next.activeModelIndex, false, firstInstruction || undefined);
  }, [startRun, resetEnsemble]);

  useEffect(() => {
    let disposed = false;
    const unlistenOpen = listen<unknown>(PANEL_OPEN_EVENT, (event) => applyInit(event.payload));
    const unlistenResult = listen<unknown>(PANEL_RESULT_EVENT, (event) => {
      const result = normalizePanelResult(event.payload);
      if (!result || result.sessionId !== sessionRef.current) return;
      // Ensemble members carry a run id and member index; they never touch the single view.
      if (handleEnsembleResult(result) || staleRef.current.has(result.requestId)) return;
      const key = resultKey(result.presetId, result.modelIndex);
      setResults((current) => ({ ...current, [key]: result }));
    });
    getPanelInit()
      .then((payload) => {
        if (!disposed && payload) applyInit(payload);
      })
      .catch(console.error);
    return () => {
      disposed = true;
      unlistenOpen.then((unlisten) => unlisten()).catch(console.error);
      unlistenResult.then((unlisten) => unlisten()).catch(console.error);
    };
  }, [applyInit, handleEnsembleResult]);

  useEffect(() => {
    if (focusInputToken > 0) inputRef.current?.focus();
  }, [focusInputToken]);

  const presets = useMemo(() => init?.presets ?? [], [init]);
  const models = useMemo(() => init?.models ?? [], [init]);
  const model = models[modelIndex] ?? null;
  const custom = isCustomPreset(presetId);
  const key = resultKey(presetId, modelIndex);
  const result = results[key] ?? null;
  const awaitingInstruction = custom && !result && !lastCustom;
  const phase = result?.phase ?? (awaitingInstruction ? null : "thinking");
  const hasText = !!result && result.text.length > 0 && (result.phase === "streaming" || result.phase === "done");
  const applicable = canApply(result) && !applying;

  const diff = useMemo(
    () => (showDiff && hasText && result ? wordDiff(init?.original ?? "", result.text) : null),
    [showDiff, hasText, result, init],
  );

  const profiles = useMemo(() => init?.ensembleProfiles ?? [], [init]);
  const mode = ensemble.mode;
  const run = ensemble.run;
  const cards = cardCount(run, ensemble.results);
  const finished = finishedCount(ensemble.results);
  const selectedCard = ensemble.results[ensemble.selected] ?? null;
  const cardHasText = !!selectedCard && selectedCard.text.length > 0 && (selectedCard.phase === "streaming" || selectedCard.phase === "done");
  const cardDiff = useMemo(
    () => (mode === "cards" && ensemble.showDiff && cardHasText && selectedCard ? wordDiff(init?.original ?? "", selectedCard.text) : null),
    [mode, ensemble.showDiff, cardHasText, selectedCard, init],
  );
  const preview = ensemble.preview;
  const previewReady = preview?.status === "ready" && preview.profileId === ensemble.profileId ? preview.preview : null;
  const runnable = !!previewReady && !previewReady.blocked && previewReady.members.length > 0 && !applying;

  const selectPreset = (id: string) => {
    if (applying || !presets.some((preset) => preset.id === id)) return;
    if (isCustomPreset(id)) {
      setPresetId(id);
      setDraft(lastCustom ?? "");
      setHistoryIndex(-1);
      setFocusInputToken((token) => token + 1);
      if (id !== presetId && lastCustom) startRun(id, modelIndex, false);
      return;
    }
    if (id === presetId) return;
    setPresetId(id);
    startRun(id, modelIndex, false);
  };

  const switchModel = (delta: 1 | -1) => {
    if (applying || models.length < 2) return;
    const next = nextModelIndex(modelIndex, models.length, delta);
    setModelIndex(next);
    startRun(presetId, next, false);
  };

  const anotherTake = () => {
    if (applying) return;
    if (custom && !lastCustom) {
      setFocusInputToken((token) => token + 1);
      return;
    }
    startRun(presetId, modelIndex, true);
  };

  const applyResult = (target: PanelResult | null) => {
    if (applying || !canApply(target) || !target) return false;
    setApplying(true);
    panelApply(target.requestId).catch((error: unknown) => {
      setApplying(false);
      showNotice(`Could not apply: ${errorMessage(error)}`);
    });
    return true;
  };

  const copyResult = (target: PanelResult | null) => {
    if (!canApply(target) || !target) return;
    panelCopy(target.requestId)
      .then(() => showNotice("Copied"))
      .catch((error: unknown) => showNotice(`Could not copy: ${errorMessage(error)}`));
  };

  const apply = () => applyResult(result);

  const close = () => {
    panelClose().catch(console.error);
  };

  const openEnsemble = () => {
    if (applying) return;
    if (profiles.length === 0) {
      showNotice("No ensemble models yet. Add favorites or a profile under Models, Text models.");
      return;
    }
    let instruction: string | null = null;
    if (custom) {
      instruction = lastCustom;
      if (!instruction) {
        showNotice("Run a custom instruction first, then press E");
        setFocusInputToken((token) => token + 1);
        return;
      }
    }
    ensemble.open(profiles, presetId, instruction);
  };

  const confirmEnsemble = () => {
    if (!runnable) return false;
    return ensemble.confirm(profiles);
  };

  const backToSingle = () => {
    ensemble.setMode("single");
    window.requestAnimationFrame(() => bodyRef.current?.focus());
  };

  const selectCard = (index: number) => {
    if (index >= 0 && index < cards) ensemble.setSelected(index);
  };

  const runCustom = () => {
    const instruction = draft.trim();
    if (!instruction || applying) return;
    setHistory((current) => rememberInstruction(current, instruction));
    setHistoryIndex(-1);
    startRun(presetId, modelIndex, true, instruction);
    bodyRef.current?.focus();
  };

  const onInputKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      runCustom();
    } else if (event.key === "ArrowUp" || event.key === "ArrowDown") {
      if (history.length === 0) return;
      event.preventDefault();
      const nextIndex = event.key === "ArrowUp"
        ? Math.min(history.length - 1, historyIndex + 1)
        : Math.max(-1, historyIndex - 1);
      setHistoryIndex(nextIndex);
      setDraft(nextIndex === -1 ? "" : history[nextIndex]);
    }
  };

  const onWindowKeyDown = (event: KeyboardEvent) => {
    if (!init || event.isComposing) return;
    if (event.key === "Escape") {
      event.preventDefault();
      // Preview and cards return to the single result first; Esc there closes.
      if (mode !== "single") backToSingle();
      else close();
      return;
    }
    const target = event.target as HTMLElement | null;
    if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)) return;
    if (applying) return;
    const lower = event.key.length === 1 ? event.key.toLowerCase() : event.key;
    if (event.ctrlKey && !event.altKey && !event.metaKey && lower === "c") {
      // A text selection inside the panel uses the native copy.
      if ((window.getSelection()?.toString() ?? "").length > 0) return;
      event.preventDefault();
      if (mode === "single") copyResult(result);
      else if (mode === "cards") copyResult(selectedCard);
      return;
    }
    if (event.ctrlKey || event.altKey || event.metaKey) return;
    if (mode === "preview") {
      onPreviewKey(event, lower);
      return;
    }
    if (mode === "cards") {
      onCardsKey(event, lower);
      return;
    }
    if (event.key === "Tab") {
      event.preventDefault();
      switchModel(event.shiftKey ? -1 : 1);
      return;
    }
    if (/^[1-9]$/.test(event.key)) {
      const preset = presets[Number(event.key) - 1];
      if (preset) {
        event.preventDefault();
        selectPreset(preset.id);
      }
      return;
    }
    if (event.key === "Enter") {
      if (apply()) event.preventDefault();
      return;
    }
    if (event.shiftKey) return;
    if (lower === "r") {
      event.preventDefault();
      anotherTake();
    } else if (lower === "d") {
      event.preventDefault();
      setShowDiff((current) => !current);
    } else if (lower === "e") {
      event.preventDefault();
      openEnsemble();
    }
  };

  const onPreviewKey = (event: KeyboardEvent, lower: string) => {
    if (/^[1-9]$/.test(event.key)) {
      const profile = profiles[Number(event.key) - 1];
      if (profile) {
        event.preventDefault();
        if (profile.id !== ensemble.profileId) ensemble.selectProfile(profile.id);
      }
      return;
    }
    if (event.key === "Enter" || (lower === "e" && !event.shiftKey)) {
      event.preventDefault();
      confirmEnsemble();
    }
  };

  const onCardsKey = (event: KeyboardEvent, lower: string) => {
    if (/^[1-9]$/.test(event.key)) {
      const index = Number(event.key) - 1;
      if (index < cards) {
        event.preventDefault();
        selectCard(index);
      }
      return;
    }
    if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
      event.preventDefault();
      selectCard(ensemble.selected + (event.key === "ArrowRight" ? 1 : -1));
      return;
    }
    if (event.key === "Enter") {
      if (applyResult(selectedCard)) event.preventDefault();
      return;
    }
    if (event.shiftKey) return;
    if (lower === "d") {
      event.preventDefault();
      ensemble.setShowDiff((current) => !current);
    } else if (lower === "r") {
      event.preventDefault();
      ensemble.rerun();
    }
  };

  const keyHandlerRef = useRef(onWindowKeyDown);
  useLayoutEffect(() => {
    keyHandlerRef.current = onWindowKeyDown;
  });
  useEffect(() => {
    const listener = (event: KeyboardEvent) => keyHandlerRef.current(event);
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);

  const color = safeColor(result?.color || model?.color);
  const style: PanelStyle = { "--model-color": color };
  const modelLabel = result?.label || model?.label || "Model";
  const effort = (result ? result.effort : null) ?? model?.effort ?? null;
  const billing = billingLabel(result?.billing || model?.billing);
  const duration = result?.phase === "done" ? formatPanelDuration(result.durationMs) : null;
  const cost = result?.phase === "done" ? formatPanelCost(result.costUsd) : null;
  const meta = [billing, cost, duration].filter(Boolean).join(" · ");
  const appName = targetAppName(init?.targetApp ?? null);
  const warnings = result && (result.phase === "done" || result.phase === "streaming") ? friendlyWarnings(result.warnings) : [];
  const status = mode === "preview"
    ? previewStatusText(preview, previewReady)
    : mode === "cards"
      ? cardsStatusText(run?.error ?? null, finished, cards, ensemble.selected, selectedCard, applying)
      : statusText(phase, result, awaitingInstruction, applying);
  const presetName = (id: string | undefined) => presets.find((preset) => preset.id === id)?.name ?? "";

  const closeButton = (
    <button type="button" className="ap-close" aria-label="Close" aria-keyshortcuts="Escape" title="Close (Esc)" onClick={close}>
      <X size={14} aria-hidden="true" />
    </button>
  );
  const liveRegion = (
    <div className="ap-sr-only" role="status" aria-live="polite">
      {notice?.text ?? status}
    </div>
  );
  const noticeText = notice && <span className="ap-notice" aria-hidden="true">{notice.text}</span>;

  let content;
  if (!init) {
    content = (
      <div className="ap-loading" role="status">
        <Loader2 className="ap-spin" size={16} aria-hidden="true" />
        Waiting for the selection…
      </div>
    );
  } else if (mode === "preview") {
    const memberCount = previewReady?.members.length ?? 0;
    content = (
      <>
        <div className="ap-topline">
          <h1 className="ap-ens-title">
            <span className="ap-ens-badge">Ensemble</span>
            <span className="ap-ens-sub">{presetName(presetId)}</span>
          </h1>
          {closeButton}
        </div>
        {profiles.length > 1 && (
          <ProfileChips profiles={profiles} profileId={ensemble.profileId} disabled={applying} onSelect={ensemble.selectProfile} />
        )}
        <div
          ref={bodyRef}
          className="ap-body"
          role="region"
          aria-label="Ensemble preview"
          aria-busy={preview?.status === "loading"}
          tabIndex={0}
        >
          <EnsemblePreviewView state={preview} />
        </div>
        {liveRegion}
        <footer className="ap-footer">
          {profiles.length > 1 && <span className="ap-hint"><kbd>{presetKeyRange(profiles.length)}</kbd> profile</span>}
          {!previewReady?.blocked && (
            <button type="button" className="ap-hint" onClick={confirmEnsemble} disabled={!runnable} aria-keyshortcuts="Enter E">
              <kbd>Enter</kbd> run {memberCount > 0 ? `${memberCount} model${memberCount === 1 ? "" : "s"}` : "ensemble"}
            </button>
          )}
          <button type="button" className="ap-hint" onClick={backToSingle} aria-keyshortcuts="Escape">
            <kbd>Esc</kbd> back
          </button>
          {noticeText}
        </footer>
      </>
    );
  } else if (mode === "cards" && run) {
    content = (
      <>
        <div className="ap-topline">
          <h1 className="ap-ens-title">
            <span className="ap-ens-badge">Ensemble · {finished} / {cards}</span>
            <span className="ap-ens-sub">{[presetName(run.presetId), run.profileName].filter(Boolean).join(" · ")}</span>
          </h1>
          {closeButton}
        </div>
        {run.error ? (
          <div ref={bodyRef} className="ap-body" role="region" aria-label="Ensemble" tabIndex={0}>
            <div className="ap-failed">
              <strong>Could not start the ensemble</strong>
              <span>{run.error}</span>
              <span className="ap-retry">Press R to try again or Esc to go back.</span>
            </div>
          </div>
        ) : (
          <EnsembleCards
            members={run.members}
            count={cards}
            results={ensemble.results}
            selected={ensemble.selected}
            showDiff={ensemble.showDiff}
            diff={cardDiff}
            disabled={applying}
            onSelect={selectCard}
          />
        )}
        {liveRegion}
        <footer className="ap-footer">
          <span className="ap-hint"><kbd>{cardKeyRange(cards)}</kbd> select</span>
          <button type="button" className="ap-hint" onClick={() => applyResult(selectedCard)} disabled={!canApply(selectedCard) || applying} aria-keyshortcuts="Enter">
            <kbd>Enter</kbd> apply
          </button>
          <button
            type="button"
            className="ap-hint"
            onClick={() => ensemble.setShowDiff((current) => !current)}
            aria-pressed={ensemble.showDiff}
            aria-keyshortcuts="D"
          >
            <kbd>D</kbd> diff
          </button>
          <button type="button" className="ap-hint" onClick={() => ensemble.rerun()} disabled={applying} aria-keyshortcuts="R">
            <kbd>R</kbd> again
          </button>
          <button type="button" className="ap-hint" onClick={backToSingle} aria-keyshortcuts="Escape">
            <kbd>Esc</kbd> back
          </button>
          {noticeText}
        </footer>
      </>
    );
  } else {
    content = (
          <>
            <div className="ap-topline">
              <div className="ap-chips" role="toolbar" aria-label="Rewrite style">
                {presets.map((preset, index) => (
                  <button
                    key={preset.id}
                    type="button"
                    className={preset.id === presetId ? "ap-chip on" : "ap-chip"}
                    aria-pressed={preset.id === presetId}
                    aria-keyshortcuts={String(index + 1)}
                    onClick={() => selectPreset(preset.id)}
                  >
                    <b>{index + 1}</b>{preset.name}
                  </button>
                ))}
              </div>
              {closeButton}
            </div>

            {custom && (
              <div className="ap-custom">
                <input
                  ref={inputRef}
                  type="text"
                  aria-label="Custom instruction"
                  placeholder="Describe the rewrite, then press Enter"
                  maxLength={CUSTOM_INSTRUCTION_MAX_CHARS}
                  value={draft}
                  onChange={(event) => {
                    setDraft(event.target.value);
                    setHistoryIndex(-1);
                  }}
                  onKeyDown={onInputKeyDown}
                />
                {history.length > 0 && <span className="ap-custom-hint">↑↓ recent</span>}
              </div>
            )}

            <div className="ap-header" data-testid="panel-header">
              <span className="ap-dot" aria-hidden="true" />
              <span className="ap-model">
                {modelLabel}
                {effort && <span className="ap-effort"> · {effort}</span>}
              </span>
              {models.length > 1 && <span className="ap-model-count">{modelIndex + 1}/{models.length}</span>}
              {meta && <span className="ap-meta">{meta}</span>}
              {appName && <span className="ap-app" title="Target app">→ {appName}</span>}
            </div>

            <div
              ref={bodyRef}
              className="ap-body"
              role="region"
              aria-label={showDiff ? "Changes against the original" : "Rewritten text"}
              aria-busy={phase === "thinking" || phase === "streaming"}
              tabIndex={0}
              data-phase={phase ?? "idle"}
            >
              <Body
                phase={phase}
                result={result}
                hasText={hasText}
                awaitingInstruction={awaitingInstruction}
                showDiff={showDiff}
                diff={diff}
              />
            </div>

            <WarningList warnings={warnings} />

            {liveRegion}

            <footer className="ap-footer">
              <span className="ap-hint"><kbd>{presetKeyRange(presets.length)}</kbd> preset</span>
              <button type="button" className="ap-hint" onClick={() => switchModel(1)} disabled={models.length < 2 || applying} aria-keyshortcuts="Tab">
                <kbd>Tab</kbd> model
              </button>
              <button type="button" className="ap-hint" onClick={anotherTake} disabled={applying} aria-keyshortcuts="R">
                <kbd>R</kbd> again
              </button>
              <button
                type="button"
                className="ap-hint"
                onClick={() => setShowDiff((current) => !current)}
                aria-pressed={showDiff}
                aria-keyshortcuts="D"
              >
                <kbd>D</kbd> diff
              </button>
              <button type="button" className="ap-hint" onClick={() => apply()} disabled={!applicable} aria-keyshortcuts="Enter">
                <kbd>Enter</kbd> apply
              </button>
              <button type="button" className="ap-hint" onClick={openEnsemble} disabled={applying || profiles.length === 0} aria-keyshortcuts="E">
                <kbd>E</kbd> ensemble
              </button>
              <button type="button" className="ap-hint" onClick={close} aria-keyshortcuts="Escape">
                <kbd>Esc</kbd> close
              </button>
              {noticeText}
            </footer>
          </>
    );
  }

  return (
    <main className="ap-root" aria-label="Rewrite panel" style={style} data-mode={mode}>
      <div className="ap-card">{content}</div>
    </main>
  );
}

function previewStatusText(preview: PreviewState | null, ready: EnsemblePreview | null) {
  if (!preview || preview.status === "loading") return "Estimating the ensemble cost";
  if (preview.status === "error") return `Ensemble unavailable: ${preview.error}`;
  if (!ready) return "";
  if (ready.blocked) return "Over the cost limit. This ensemble cannot run.";
  return `Press Enter to run ${ready.members.length} models`;
}

function cardsStatusText(error: string | null, finished: number, count: number, selected: number, card: PanelResult | null, applying: boolean) {
  if (applying) return "Applying";
  if (error) return `Could not start the ensemble: ${error}`;
  const phase = card?.phase ?? "thinking";
  const selectedText = phase === "done"
    ? canApply(card) ? "done, press Enter to apply" : "done, but empty"
    : phase === "failed" ? `failed: ${card?.error || "request failed"}` : phase === "streaming" ? "writing" : phase;
  return `${finished} of ${count} finished. Card ${selected + 1} ${selectedText}`;
}

function statusText(phase: PanelResult["phase"] | null, result: PanelResult | null, awaiting: boolean, applying: boolean) {
  if (applying) return "Applying";
  if (awaiting) return "Type an instruction and press Enter";
  switch (phase) {
    case "thinking":
      return "Thinking";
    case "streaming":
      return "Writing";
    case "done":
      return canApply(result) ? "Done. Press Enter to apply" : "Done, but the result is empty";
    case "failed":
      return `Failed: ${result?.error || "request failed"}`;
    case "cancelled":
      return "Cancelled";
    default:
      return "";
  }
}

type BodyProps = {
  phase: PanelResult["phase"] | null;
  result: PanelResult | null;
  hasText: boolean;
  awaitingInstruction: boolean;
  showDiff: boolean;
  diff: DiffPart[] | null;
};

function Body({ phase, result, hasText, awaitingInstruction, showDiff, diff }: BodyProps) {
  if (awaitingInstruction) {
    return <p className="ap-placeholder">Type an instruction above and press Enter.</p>;
  }
  if (phase === "failed") {
    return (
      <div className="ap-failed">
        <strong>Failed</strong>
        <span>{result?.error || "Request failed"}</span>
        <span className="ap-retry">Press R to try again.</span>
      </div>
    );
  }
  if (phase === "cancelled") {
    return <p className="ap-placeholder">Cancelled. Press R to try again.</p>;
  }
  if (!hasText || !result) {
    if (phase === "done") return <p className="ap-placeholder">The model returned no text. Press R to try again.</p>;
    return (
      <div className="ap-thinking">
        <Loader2 className="ap-spin" size={16} aria-hidden="true" />
        Thinking…
      </div>
    );
  }
  return (
    <ResultText
      text={result.text}
      diff={diff}
      showDiff={showDiff}
      streaming={phase === "streaming"}
      testId="panel-text"
      diffTestId="panel-diff"
    />
  );
}
