import { useEffect, useRef, useState } from "react";
import { Clipboard, Play, Trash2 } from "lucide-react";
import { copyTranscript, deleteTranscript, pasteTranscript } from "../lib/api";
import type { TranscriptHistoryItem } from "../lib/types";

type Props = { item: TranscriptHistoryItem; allowDelete?: boolean };
let scheduledPaste: { owner: object; cancel: () => void } | null = null;

export function TranscriptActions({ item, allowDelete = false }: Props) {
  const [busy, setBusy] = useState(false);
  const [countdown, setCountdown] = useState<number | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [feedback, setFeedback] = useState<{ text: string; error: boolean } | null>(null);
  const timer = useRef<number | null>(null);
  const mounted = useRef(true);
  const owner = useRef({});
  const currentItemId = useRef(item.id);
  const deleteButton = useRef<HTMLButtonElement>(null);
  const cancelDeleteButton = useRef<HTMLButtonElement>(null);

  function cancelPaste() {
    if (timer.current !== null) window.clearInterval(timer.current);
    timer.current = null;
    if (scheduledPaste?.owner === owner.current) scheduledPaste = null;
    setCountdown(null);
  }

  useEffect(() => {
    mounted.current = true;
    currentItemId.current = item.id;
    setBusy(false);
    setCountdown(null);
    setConfirmDelete(false);
    setFeedback(null);
    const pasteOwner = owner.current;
    return () => {
      mounted.current = false;
      if (timer.current !== null) window.clearInterval(timer.current);
      timer.current = null;
      if (scheduledPaste?.owner === pasteOwner) scheduledPaste = null;
    };
  }, [item.id]);

  useEffect(() => {
    if (confirmDelete) cancelDeleteButton.current?.focus();
  }, [confirmDelete]);

  async function perform(action: "copy" | "paste" | "delete") {
    setBusy(true);
    setFeedback(null);
    try {
      if (action === "copy") await copyTranscript(item.id);
      if (action === "paste") await pasteTranscript(item.id);
      if (action === "delete") await deleteTranscript(item.id);
      if (mounted.current && currentItemId.current === item.id) {
        setFeedback({
          text: action === "copy" ? "Copied to clipboard." : action === "paste" ? "Paste sent to the focused app." : "Transcript deleted.",
          error: false,
        });
        setConfirmDelete(false);
      }
    } catch (error) {
      if (mounted.current && currentItemId.current === item.id) setFeedback({ text: `Could not ${action}: ${String(error)}`, error: true });
    } finally {
      if (mounted.current && currentItemId.current === item.id) setBusy(false);
    }
  }

  function schedulePaste() {
    if (busy || timer.current !== null) return;
    scheduledPaste?.cancel();
    scheduledPaste = { owner: owner.current, cancel: cancelPaste };
    setFeedback(null);
    setConfirmDelete(false);
    setCountdown(3);
    let remaining = 3;
    timer.current = window.setInterval(() => {
      remaining -= 1;
      if (remaining === 0) {
        cancelPaste();
        void perform("paste");
      } else {
        setCountdown(remaining);
      }
    }, 1000);
  }

  return (
    <div className="transcript-controls">
      <div className="history-actions">
        <button onClick={() => void perform("copy")} disabled={busy || countdown !== null}>
          <Clipboard size={14} aria-hidden="true" /> Copy
        </button>
        {countdown !== null ? (
          <button onClick={cancelPaste}>Cancel paste ({countdown}s)</button>
        ) : (
          <button onClick={schedulePaste} disabled={busy} title="You have three seconds to focus the app where you want to paste.">
            <Play size={14} aria-hidden="true" /> Paste in 3s
          </button>
        )}
        {allowDelete && (
          <button ref={deleteButton} className="danger-text" onClick={() => setConfirmDelete(true)} disabled={busy || countdown !== null || confirmDelete}>
            <Trash2 size={14} aria-hidden="true" /> Delete
          </button>
        )}
      </div>
      {countdown !== null && <p className="action-feedback" role="status">Switch to the destination app. Pasting in {countdown} seconds.</p>}
      {confirmDelete && (
        <div className="inline-confirmation" role="group" aria-label="Confirm transcript deletion">
          <span>Delete this transcript permanently?</span>
          <button className="danger-text" onClick={() => void perform("delete")} disabled={busy}>Delete transcript</button>
          <button ref={cancelDeleteButton} onClick={() => { setConfirmDelete(false); window.requestAnimationFrame(() => deleteButton.current?.focus()); }} disabled={busy}>Cancel</button>
        </div>
      )}
      {feedback && <p className={`action-feedback ${feedback.error ? "error-text" : "success-text"}`} role={feedback.error ? "alert" : "status"}>{feedback.text}</p>}
    </div>
  );
}
