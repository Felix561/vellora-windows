import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Clipboard, RefreshCw, Trash2 } from "lucide-react";
import {
  clearTextAssistRuns,
  copyTextAssistText,
  deleteTextAssistRun,
  getTextAssistOverview,
  listTextAssistRuns,
  onTextAssistRunRecorded,
} from "../lib/api";
import { formatDuration } from "../lib/format";
import { effortLabel, formatRunCost, kindLabel, outcomeLabel, reasonText, runCopyText } from "../lib/textAssist";
import type { TextAssistRun } from "../lib/types";
import { countChanges, wordDiff, type DiffPart } from "../lib/wordDiff";

type Props = {
  /** Whether Text Assist history saving is on (`settings.textAssist.saveHistory`). */
  saveHistory: boolean;
};

type Feedback = { text: string; error: boolean } | null;

const RUN_LIMIT = 100;

function timestamp(value: string) {
  const time = Date.parse(value);
  return Number.isNaN(time) ? 0 : time;
}

function formatTime(value: string) {
  const time = timestamp(value);
  return time ? new Date(time).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }) : value;
}

function preview(text: string) {
  const line = text.replace(/\s+/g, " ").trim();
  return line.length > 140 ? `${line.slice(0, 139)}…` : line;
}

function showsReason(run: TextAssistRun) {
  return run.outcome === "kept" || run.outcome === "failed";
}

export function TextAssistHistoryView({ saveHistory }: Props) {
  const [runs, setRuns] = useState<TextAssistRun[]>([]);
  const [lastKept, setLastKept] = useState<TextAssistRun | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [message, setMessage] = useState<Feedback>(null);
  const requestId = useRef(0);
  const mounted = useRef(true);
  const clearButton = useRef<HTMLButtonElement>(null);
  const cancelClearButton = useRef<HTMLButtonElement>(null);
  const deleteButton = useRef<HTMLButtonElement>(null);
  const cancelDeleteButton = useRef<HTMLButtonElement>(null);
  const listHeading = useRef<HTMLHeadingElement>(null);

  const load = useCallback(async () => {
    // Each load (and each mutation) supersedes earlier reads, so a slow
    // response cannot bring back a run that was deleted meanwhile.
    const id = ++requestId.current;
    const [nextRuns, overview] = await Promise.allSettled([listTextAssistRuns(RUN_LIMIT), getTextAssistOverview()]);
    if (!mounted.current || id !== requestId.current) return;
    if (nextRuns.status === "fulfilled") {
      setRuns(nextRuns.value);
      setLoadError(null);
    } else {
      setLoadError(`Could not load Text Assist history: ${String(nextRuns.reason)}`);
    }
    if (overview.status === "fulfilled") setLastKept(overview.value.lastKept);
    setLoaded(true);
  }, []);

  useEffect(() => {
    mounted.current = true;
    void load();
    const unlisten = onTextAssistRunRecorded(() => { void load(); });
    return () => {
      mounted.current = false;
      requestId.current += 1;
      unlisten.then((stop) => stop()).catch(console.error);
    };
  }, [load]);

  useEffect(() => { if (confirmClear) cancelClearButton.current?.focus(); }, [confirmClear]);
  useEffect(() => { if (confirmDelete) cancelDeleteButton.current?.focus(); }, [confirmDelete]);

  const selected = runs.find((run) => run.id === selectedId) ?? null;
  const newest = runs[0];
  const showBanner = Boolean(lastKept)
    && !runs.some((run) => run.id === lastKept?.id)
    && (!saveHistory || !newest || timestamp(lastKept?.createdAt ?? "") > timestamp(newest.createdAt));

  function select(id: string) {
    setSelectedId((current) => (current === id ? null : id));
    setConfirmDelete(false);
    setMessage(null);
  }

  async function refresh() {
    setBusy(true);
    setMessage(null);
    try { await load(); } finally { if (mounted.current) setBusy(false); }
  }

  async function copy(text: string, label: string) {
    setBusy(true);
    setMessage(null);
    try {
      await copyTextAssistText(text);
      if (mounted.current) setMessage({ text: `${label} copied to the clipboard.`, error: false });
    } catch (error) {
      if (mounted.current) setMessage({ text: `Could not copy: ${String(error)}`, error: true });
    } finally {
      if (mounted.current) setBusy(false);
    }
  }

  async function deleteRun(id: string) {
    setBusy(true);
    setMessage(null);
    try {
      await deleteTextAssistRun(id);
      if (!mounted.current) return;
      requestId.current += 1;
      setRuns((items) => items.filter((run) => run.id !== id));
      setSelectedId(null);
      setConfirmDelete(false);
      setMessage({ text: "Text Assist run deleted.", error: false });
      window.requestAnimationFrame(() => listHeading.current?.focus());
      void load();
    } catch (error) {
      if (mounted.current) setMessage({ text: `Could not delete: ${String(error)}`, error: true });
    } finally {
      if (mounted.current) setBusy(false);
    }
  }

  async function clearAll() {
    setBusy(true);
    setMessage(null);
    try {
      const count = await clearTextAssistRuns();
      if (!mounted.current) return;
      requestId.current += 1;
      setRuns([]);
      setSelectedId(null);
      setConfirmClear(false);
      setMessage({ text: `Deleted ${count} Text Assist run${count === 1 ? "" : "s"}.`, error: false });
      void load();
    } catch (error) {
      if (mounted.current) setMessage({ text: `Could not clear Text Assist history: ${String(error)}`, error: true });
    } finally {
      if (mounted.current) setBusy(false);
    }
  }

  return (
    <section className="panel history-panel" aria-labelledby="ta-history-title">
      <div className="section-title">
        <div>
          <h3 id="ta-history-title" ref={listHeading} tabIndex={-1}>Text Assist runs</h3>
          <p>{saveHistory ? `${runs.length} recent runs saved locally.` : "History saving is off. New runs are not saved."}</p>
        </div>
        <div className="history-toolbar">
          <button className="icon-button" onClick={() => void refresh()} disabled={busy} title="Refresh Text Assist history" aria-label="Refresh Text Assist history"><RefreshCw size={16} aria-hidden="true" /></button>
          <button ref={clearButton} className="icon-button danger-text" onClick={() => { setConfirmDelete(false); setConfirmClear(true); }} disabled={busy || runs.length === 0 || confirmClear}><Trash2 size={16} aria-hidden="true" /> Clear Text Assist history</button>
        </div>
      </div>

      {confirmClear && (
        <div className="inline-confirmation" role="group" aria-label="Confirm clearing Text Assist history">
          <span>Permanently delete all saved Text Assist runs, including any older than the ones shown here? This also resets the Text Assist totals on the Dashboard.</span>
          <button className="icon-button danger-text" onClick={() => void clearAll()} disabled={busy}>Delete all Text Assist runs</button>
          <button ref={cancelClearButton} className="icon-button" onClick={() => { setConfirmClear(false); window.requestAnimationFrame(() => clearButton.current?.focus()); }} disabled={busy}>Cancel</button>
        </div>
      )}
      {message && <p className={message.error ? "error-text" : "success-text"} role={message.error ? "alert" : "status"}>{message.text}</p>}
      {loadError && <p className="error-text" role="alert">{loadError} <button className="icon-button" onClick={() => void refresh()} disabled={busy}>Retry</button></p>}

      {showBanner && lastKept && (
        <div className="ta-banner" role="region" aria-label="Latest kept result">
          <div>
            <strong>Result ready from {lastKept.targetApp ?? "your last app"}</strong>
            <p>{reasonText(lastKept.reasonCode) ?? "The result was not pasted."} {!saveHistory && "It stays available until you quit Vellora."}</p>
          </div>
          <button className="icon-button" onClick={() => void copy(runCopyText(lastKept), lastKept.resultText !== null ? "Result" : "Original")} disabled={busy}>
            <Clipboard size={14} aria-hidden="true" /> Copy
          </button>
        </div>
      )}

      <div className="ta-history-layout">
        <div className="ta-run-column">
          {loaded && runs.length === 0 && !loadError && (
            <p className="empty">No Text Assist runs yet. Select text in any app and tap Shift+Win.</p>
          )}
          {runs.length > 0 && (
            <ul className="ta-run-list" aria-label="Text Assist runs">
              {runs.map((run) => (
                <li key={run.id}>
                  <button type="button" className={run.id === selectedId ? "ta-run selected" : "ta-run"}
                    aria-pressed={run.id === selectedId} aria-controls="ta-run-details" onClick={() => select(run.id)}>
                    <span className="ta-run-head">
                      <span className="ta-run-app">{run.targetApp ?? "Unknown app"}</span>
                      <OutcomeBadge outcome={run.outcome} />
                    </span>
                    <span className="ta-run-preview">{preview(run.originalText)}</span>
                    <span className="history-facts">
                      <span>{formatTime(run.createdAt)}</span>
                      <span>{run.model} · {effortLabel(run.effort)}</span>
                      <span>{kindLabel(run.kind)}</span>
                      {run.durationMs !== null && <span>{formatDuration(run.durationMs)}</span>}
                      {run.estimatedCostUsd !== null && <span>{formatRunCost(run.estimatedCostUsd)}</span>}
                    </span>
                    {showsReason(run) && run.reasonCode && <span className="ta-run-reason">{reasonText(run.reasonCode)}</span>}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>

        <div id="ta-run-details" className="ta-details-column">
          {selected ? (
            <RunDetails key={selected.id} run={selected} busy={busy}
              confirmDelete={confirmDelete}
              deleteButton={deleteButton}
              cancelDeleteButton={cancelDeleteButton}
              onCopy={() => void copy(runCopyText(selected), selected.resultText !== null ? "Result" : "Original")}
              onAskDelete={() => { setConfirmClear(false); setConfirmDelete(true); }}
              onCancelDelete={() => { setConfirmDelete(false); window.requestAnimationFrame(() => deleteButton.current?.focus()); }}
              onDelete={() => void deleteRun(selected.id)} />
          ) : runs.length > 0 ? (
            <p className="hint">Select a run to compare the original with the result.</p>
          ) : null}
        </div>
      </div>
    </section>
  );
}

function OutcomeBadge({ outcome }: { outcome: string }) {
  return (
    <span className={`ta-badge ta-badge-${outcome.replace(/[^a-z_]/g, "")}`}>
      <span className="visually-hidden">Outcome: </span>{outcomeLabel(outcome)}
    </span>
  );
}

/** Marks a changed word run; trailing whitespace stays outside the mark. */
function DiffSegment({ kind, text }: DiffPart) {
  if (kind === "same") return <span>{text}</span>;
  const core = text.trimEnd();
  const trailing = text.slice(core.length);
  const label = kind === "removed" ? "removed" : "added";
  const content = <><span className="visually-hidden">[{label}: </span>{core}<span className="visually-hidden">]</span></>;
  return <>{kind === "removed" ? <del>{content}</del> : <ins>{content}</ins>}{trailing}</>;
}

type DetailsProps = {
  run: TextAssistRun;
  busy: boolean;
  confirmDelete: boolean;
  deleteButton: React.RefObject<HTMLButtonElement>;
  cancelDeleteButton: React.RefObject<HTMLButtonElement>;
  onCopy: () => void;
  onAskDelete: () => void;
  onCancelDelete: () => void;
  onDelete: () => void;
};

function RunDetails({ run, busy, confirmDelete, deleteButton, cancelDeleteButton, onCopy, onAskDelete, onCancelDelete, onDelete }: DetailsProps) {
  const diff = useMemo(() => (run.resultText === null ? null : wordDiff(run.originalText, run.resultText)), [run.originalText, run.resultText]);
  const tokens = run.inputTokens !== null || run.outputTokens !== null
    ? `${run.inputTokens ?? "?"} in / ${run.outputTokens ?? "?"} out`
    : null;
  const reason = reasonText(run.reasonCode);

  return (
    <article className="ta-details" aria-labelledby="ta-details-title">
      <div className="section-title">
        <div>
          <h4 id="ta-details-title">Run details</h4>
          <p>{formatTime(run.createdAt)} · {run.targetApp ?? "Unknown app"}</p>
        </div>
        <OutcomeBadge outcome={run.outcome} />
      </div>
      <div className="kv compact">
        <span>Model</span><strong>{run.model} · {effortLabel(run.effort)}</strong>
        <span>Kind</span><strong>{kindLabel(run.kind)}</strong>
        {run.durationMs !== null && <><span>Duration</span><strong>{formatDuration(run.durationMs)}</strong></>}
        {run.firstTokenMs !== null && <><span>First text after</span><strong>{formatDuration(run.firstTokenMs)}</strong></>}
        {tokens && <><span>Tokens</span><strong>{tokens}</strong></>}
        {run.estimatedCostUsd !== null && <><span>Estimated cost</span><strong>{formatRunCost(run.estimatedCostUsd)}</strong></>}
      </div>
      {reason && run.outcome !== "applied" && run.outcome !== "no_change" && <p className={run.outcome === "failed" ? "error-text" : "hint"}>{reason}</p>}

      {run.resultText !== null && diff && (
        <div className="ta-diff-block">
          <h5>Changes <span className="ta-change-count">({countChanges(diff)} {countChanges(diff) === 1 ? "change" : "changes"})</span></h5>
          <p className="ta-diff" aria-label="Word changes from original to result">
            {diff.map((part, index) => <DiffSegment key={index} kind={part.kind} text={part.text} />)}
          </p>
        </div>
      )}
      {run.resultText !== null && !diff && <p className="hint">This text is too long for a word-by-word comparison. The original and result are shown separately.</p>}

      <div className="ta-compare">
        <div>
          <h5>Original</h5>
          <p className="transcript-preview">{run.originalText}</p>
        </div>
        <div>
          <h5>Result</h5>
          {run.resultText !== null ? <p className="transcript-preview">{run.resultText}</p> : <p className="hint">No result was produced.</p>}
        </div>
      </div>

      <div className="transcript-controls">
        <div className="history-actions">
          <button onClick={onCopy} disabled={busy}>
            <Clipboard size={14} aria-hidden="true" /> {run.resultText !== null ? "Copy result" : "Copy original"}
          </button>
          <button ref={deleteButton} className="danger-text" onClick={onAskDelete} disabled={busy || confirmDelete}>
            <Trash2 size={14} aria-hidden="true" /> Delete
          </button>
        </div>
        {confirmDelete && (
          <div className="inline-confirmation" role="group" aria-label="Confirm Text Assist run deletion">
            <span>Delete this Text Assist run permanently?</span>
            <button className="danger-text" onClick={onDelete} disabled={busy}>Delete run</button>
            <button ref={cancelDeleteButton} onClick={onCancelDelete} disabled={busy}>Cancel</button>
          </div>
        )}
      </div>
    </article>
  );
}
