import { useEffect, useRef, useState } from "react";
import { RefreshCw, Trash2 } from "lucide-react";
import { clearTranscripts } from "../lib/api";
import type { TranscriptHistoryItem } from "../lib/types";
import { SectionTabs } from "./SectionTabs";
import { TextAssistHistoryView } from "./TextAssistHistoryView";
import { TranscriptActions } from "./TranscriptActions";

type Props = {
  items: TranscriptHistoryItem[];
  onRefresh: () => Promise<void>;
  /** `settings.textAssist.saveHistory`; decides when the "Result ready" banner appears. */
  textAssistSaveHistory?: boolean;
};

type HistoryTab = "dictation" | "textAssist";

const historyTabs: ReadonlyArray<{ id: HistoryTab; label: string }> = [
  { id: "dictation", label: "Dictation" },
  { id: "textAssist", label: "Text Assist" },
];

export function HistoryView({ items, onRefresh, textAssistSaveHistory = true }: Props) {
  const [tab, setTab] = useState<HistoryTab>("dictation");
  return (
    <div className="history-stack">
      <SectionTabs idPrefix="history" label="History sections" tabs={historyTabs} selected={tab} onSelect={setTab} />
      {tab === "dictation" ? (
        <div role="tabpanel" id="history-panel-dictation" aria-labelledby="history-tab-dictation">
          <DictationHistory items={items} onRefresh={onRefresh} />
        </div>
      ) : (
        <div role="tabpanel" id="history-panel-textAssist" aria-labelledby="history-tab-textAssist">
          <TextAssistHistoryView saveHistory={textAssistSaveHistory} />
        </div>
      )}
    </div>
  );
}

function DictationHistory({ items, onRefresh }: Omit<Props, "textAssistSaveHistory">) {
  const [busy, setBusy] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);
  const clearButton = useRef<HTMLButtonElement>(null);
  const cancelClearButton = useRef<HTMLButtonElement>(null);
  useEffect(() => { if (confirmClear) cancelClearButton.current?.focus(); }, [confirmClear]);

  async function refreshHistory() {
    setBusy(true);
    setMessage(null);
    try { await onRefresh(); }
    catch (error) { setMessage({ text: `Could not refresh history: ${String(error)}`, error: true }); }
    finally { setBusy(false); }
  }

  async function clearHistory() {
    setBusy(true);
    setMessage(null);
    try {
      const count = await clearTranscripts();
      setConfirmClear(false);
      await onRefresh();
      setMessage({ text: `Deleted ${count} saved transcript${count === 1 ? "" : "s"}.`, error: false });
    } catch (error) { setMessage({ text: `Could not clear history: ${String(error)}`, error: true }); }
    finally { setBusy(false); }
  }

  return (
    <section className="panel history-panel">
      <div className="section-title">
        <div>
          <h3>Recent transcriptions</h3>
          <p>{items.length} recent transcripts loaded locally. Audio is never saved in history.</p>
        </div>
        <div className="history-toolbar">
          <button className="icon-button" onClick={() => void refreshHistory()} disabled={busy} title="Refresh history" aria-label="Refresh history"><RefreshCw size={16} aria-hidden="true" /></button>
          <button ref={clearButton} className="icon-button danger-text" onClick={() => setConfirmClear(true)} disabled={busy || items.length === 0 || confirmClear}><Trash2 size={16} aria-hidden="true" /> Clear all history</button>
        </div>
      </div>

      {confirmClear && <div className="inline-confirmation" role="group" aria-label="Confirm clearing history"><span>Permanently delete all saved transcripts, including any older than the ones shown here? This also resets history-based statistics.</span><button className="icon-button danger-text" onClick={() => void clearHistory()} disabled={busy}>Delete all transcripts</button><button ref={cancelClearButton} className="icon-button" onClick={() => { setConfirmClear(false); window.requestAnimationFrame(() => clearButton.current?.focus()); }} disabled={busy}>Cancel</button></div>}
      {message && <p className={message.error ? "error-text" : "success-text"} role={message.error ? "alert" : "status"}>{message.text}</p>}

      <div className="history-list">
        {items.length === 0 && (
          <p className="empty">No leaves yet. Your saved dictations will gather here as text, never audio.</p>
        )}
        {items.map((item) => (
          <article className="history-item" key={item.id}>
            <p>{item.finalText}</p>
            <footer className="history-meta">
              <div className="history-facts">
                <span>{new Date(item.createdAt).toLocaleString()}</span>
                <span>{item.wordCount} words</span>
                <span>{item.charCount} chars</span>
                <span>{item.cleanupMode === "light" ? "cleaned" : "raw"}</span>
                <span className="chip">{item.sttModel}</span>
                {item.transcriptionDurationMs && <span>{formatMs(item.transcriptionDurationMs)}</span>}
              </div>
              <TranscriptActions item={item} allowDelete />
              {item.error && <div className="error-text">{item.error} You can copy the transcript above to recover it.</div>}
            </footer>
          </article>
        ))}
      </div>
    </section>
  );
}

function formatMs(value: number) {
  if (value < 1000) return `${value}ms`;
  return `${(value / 1000).toFixed(1)}s`;
}
