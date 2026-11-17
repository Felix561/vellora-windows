// Ensemble state for the Rewrite panel (spec 2.3, M7): preview, run and one card per member.
import { useCallback, useRef, useState, type MutableRefObject } from "react";
import {
  MAX_ENSEMBLE_MEMBERS,
  normalizeEnsemblePreview,
  panelEnsemblePreview,
  panelRunEnsemble,
  type EnsemblePreview,
  type PanelEnsembleProfile,
  type PanelModel,
  type PanelResult,
} from "./assistPanel";

export type PanelMode = "single" | "preview" | "cards";

export type PreviewState =
  | { status: "loading"; profileId: string }
  | { status: "ready"; profileId: string; preview: EnsemblePreview }
  | { status: "error"; profileId: string; error: string };

/** What an ensemble run (and its re-run) uses. */
export type EnsembleSpec = {
  profileId: string;
  profileName: string;
  presetId: string;
  customInstruction: string | null;
  members: PanelModel[];
};

export type EnsembleRun = EnsembleSpec & {
  token: number;
  /** Null until `panel_run_ensemble` resolves. */
  runId: string | null;
  /** Set when the run could not start (for example above the cost limit). */
  error: string | null;
};

/** At most this many runs' early events are buffered while the run id is still unknown. */
const MAX_BUFFERED_RUNS = 8;

function errorMessage(error: unknown) {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "Something went wrong";
}

/** Number of cards: the profile's members, plus any member index the backend reported beyond it. */
export function cardCount(run: EnsembleRun | null, results: Record<number, PanelResult>) {
  if (!run) return 0;
  const highest = Object.keys(results).reduce((max, key) => Math.max(max, Number(key) + 1), 0);
  return Math.min(MAX_ENSEMBLE_MEMBERS, Math.max(run.members.length, highest));
}

/** Members that reached a final phase. */
export function finishedCount(results: Record<number, PanelResult>) {
  return Object.values(results).filter((result) => result.phase === "done" || result.phase === "failed" || result.phase === "cancelled").length;
}

/**
 * Ensemble mode of the panel. Member events (`ensembleRunId` + `memberIndex`) are routed here by
 * `handleResult`; events of other runs or sessions are ignored. Events that arrive before
 * `panel_run_ensemble` returns the run id are buffered and adopted once it is known.
 */
export function useEnsemble(sessionRef: MutableRefObject<string | null>) {
  const [mode, setMode] = useState<PanelMode>("single");
  const [profileId, setProfileId] = useState("");
  const [preview, setPreview] = useState<PreviewState | null>(null);
  const [run, setRun] = useState<EnsembleRun | null>(null);
  const [results, setResults] = useState<Record<number, PanelResult>>({});
  const [selected, setSelected] = useState(0);
  const [showDiff, setShowDiff] = useState(false);

  /** Preset and instruction captured when the preview opened. */
  const specRef = useRef<{ presetId: string; customInstruction: string | null } | null>(null);
  const previewToken = useRef(0);
  const runToken = useRef(0);
  const activeRef = useRef<{ token: number; runId: string | null } | null>(null);
  const bufferRef = useRef(new Map<string, Map<number, PanelResult>>());

  const reset = useCallback(() => {
    previewToken.current += 1;
    runToken.current += 1;
    activeRef.current = null;
    bufferRef.current = new Map();
    specRef.current = null;
    setMode("single");
    setPreview(null);
    setRun(null);
    setResults({});
    setSelected(0);
    setShowDiff(false);
  }, []);

  const loadPreview = useCallback((id: string) => {
    const session = sessionRef.current;
    const spec = specRef.current;
    if (!session || !spec) return;
    const token = ++previewToken.current;
    setProfileId(id);
    setPreview({ status: "loading", profileId: id });
    panelEnsemblePreview({ profileId: id, presetId: spec.presetId, customInstruction: spec.customInstruction })
      .then((raw) => {
        if (token !== previewToken.current || sessionRef.current !== session) return;
        const value = normalizeEnsemblePreview(raw);
        setPreview(value
          ? { status: "ready", profileId: id, preview: value }
          : { status: "error", profileId: id, error: "The cost estimate is unavailable" });
      })
      .catch((error: unknown) => {
        if (token !== previewToken.current || sessionRef.current !== session) return;
        setPreview({ status: "error", profileId: id, error: errorMessage(error) });
      });
  }, [sessionRef]);

  /** Opens the preview for `presetId`; keeps the last chosen profile when it still exists. */
  const open = useCallback((profiles: readonly PanelEnsembleProfile[], presetId: string, customInstruction: string | null) => {
    if (profiles.length === 0) return;
    specRef.current = { presetId, customInstruction };
    const id = profiles.some((profile) => profile.id === profileId) ? profileId : profiles[0].id;
    setMode("preview");
    loadPreview(id);
  }, [loadPreview, profileId]);

  const start = useCallback((spec: EnsembleSpec) => {
    const session = sessionRef.current;
    if (!session) return;
    const token = ++runToken.current;
    activeRef.current = { token, runId: null };
    bufferRef.current = new Map();
    setRun({ ...spec, token, runId: null, error: null });
    setResults({});
    setSelected(0);
    setShowDiff(false);
    setMode("cards");
    panelRunEnsemble({ profileId: spec.profileId, presetId: spec.presetId, customInstruction: spec.customInstruction })
      .then((runId) => {
        if (sessionRef.current !== session || activeRef.current?.token !== token) return;
        activeRef.current = { token, runId };
        const early = bufferRef.current.get(runId);
        bufferRef.current = new Map();
        setRun((current) => (current?.token === token ? { ...current, runId } : current));
        if (early) setResults((current) => ({ ...Object.fromEntries(early), ...current }));
      })
      .catch((error: unknown) => {
        if (sessionRef.current !== session || activeRef.current?.token !== token) return;
        activeRef.current = null;
        bufferRef.current = new Map();
        setRun((current) => (current?.token === token ? { ...current, error: errorMessage(error) } : current));
      });
  }, [sessionRef]);

  /** Starts the previewed profile; false when the preview is not ready or blocked. */
  const confirm = useCallback((profiles: readonly PanelEnsembleProfile[]) => {
    const spec = specRef.current;
    if (!spec || preview?.status !== "ready" || preview.preview.blocked || preview.profileId !== profileId) return false;
    if (preview.preview.members.length === 0) return false;
    const profile = profiles.find((item) => item.id === profileId);
    start({
      profileId,
      profileName: profile?.name ?? profileId,
      presetId: spec.presetId,
      customInstruction: spec.customInstruction,
      members: preview.preview.members,
    });
    return true;
  }, [preview, profileId, start]);

  /** Runs the same profile, preset and instruction again (fresh requests). */
  const rerun = useCallback(() => {
    if (!run) return false;
    start({
      profileId: run.profileId,
      profileName: run.profileName,
      presetId: run.presetId,
      customInstruction: run.customInstruction,
      members: run.members,
    });
    return true;
  }, [run, start]);

  /** Routes a member event; returns true when the event belongs to an ensemble (handled or ignored). */
  const handleResult = useCallback((result: PanelResult) => {
    if (result.ensembleRunId === undefined || result.memberIndex === undefined) return false;
    if (result.memberIndex >= MAX_ENSEMBLE_MEMBERS) return true;
    const active = activeRef.current;
    if (!active) return true;
    if (active.runId === null) {
      const buffer = bufferRef.current;
      if (!buffer.has(result.ensembleRunId) && buffer.size >= MAX_BUFFERED_RUNS) return true;
      const members = buffer.get(result.ensembleRunId) ?? new Map<number, PanelResult>();
      members.set(result.memberIndex, result);
      buffer.set(result.ensembleRunId, members);
      return true;
    }
    if (result.ensembleRunId !== active.runId) return true;
    const index = result.memberIndex;
    setResults((current) => ({ ...current, [index]: result }));
    return true;
  }, []);

  return {
    mode,
    setMode,
    profileId,
    preview,
    run,
    results,
    selected,
    setSelected,
    showDiff,
    setShowDiff,
    reset,
    open,
    selectProfile: loadPreview,
    confirm,
    rerun,
    handleResult,
  };
}
