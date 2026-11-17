// Ensemble preview and cards for the Rewrite panel (spec 2.3, M7; concept scene C).
import { useEffect, useId, useRef, type CSSProperties } from "react";
import { AlertTriangle, Loader2 } from "lucide-react";
import {
  billingLabel,
  canApply,
  formatPanelCost,
  formatPanelDuration,
  formatUsd,
  friendlyWarnings,
  providerName,
  safeColor,
  type PanelEnsembleProfile,
  type PanelModel,
  type PanelPhase,
  type PanelResult,
} from "../lib/assistPanel";
import type { PreviewState } from "../lib/assistEnsemble";
import type { DiffPart } from "../lib/wordDiff";

type ModelStyle = CSSProperties & Record<`--${string}`, string>;

/** Result text, or a word diff when `showDiff` is on (falls back to the text when too long). */
export function ResultText({ text, diff, showDiff, streaming, testId, diffTestId }: {
  text: string;
  diff: DiffPart[] | null;
  showDiff: boolean;
  streaming: boolean;
  testId: string;
  diffTestId: string;
}) {
  if (showDiff) {
    if (!diff) {
      return (
        <>
          <p className="ap-placeholder">Too long for a word diff; showing the result.</p>
          <div className="ap-text">{text}</div>
        </>
      );
    }
    return (
      <div className="ap-text ap-diff" data-testid={diffTestId}>
        {diff.map((part, index) => {
          if (part.kind === "added") return <ins key={index} title="Added">{part.text}</ins>;
          if (part.kind === "removed") return <del key={index} title="Removed">{part.text}</del>;
          return <span key={index}>{part.text}</span>;
        })}
      </div>
    );
  }
  return (
    <div className="ap-text" data-testid={testId}>
      {text}
      {streaming && <span className="ap-caret" aria-hidden="true" />}
    </div>
  );
}

export function WarningList({ warnings, label = "Warnings" }: { warnings: string[]; label?: string }) {
  if (warnings.length === 0) return null;
  return (
    <ul className="ap-warnings" aria-label={label}>
      {warnings.map((warning) => (
        <li key={warning}>
          <AlertTriangle size={12} aria-hidden="true" />
          {warning}
        </li>
      ))}
    </ul>
  );
}

/** Profile picker shown in the preview when there are several profiles (number keys select). */
export function ProfileChips({ profiles, profileId, disabled, onSelect }: {
  profiles: readonly PanelEnsembleProfile[];
  profileId: string;
  disabled: boolean;
  onSelect: (id: string) => void;
}) {
  return (
    <div className="ap-chips ap-profile-chips" role="toolbar" aria-label="Ensemble profile">
      {profiles.map((profile, index) => (
        <button
          key={profile.id}
          type="button"
          className={profile.id === profileId ? "ap-chip on" : "ap-chip"}
          aria-pressed={profile.id === profileId}
          aria-keyshortcuts={String(index + 1)}
          disabled={disabled}
          onClick={() => onSelect(profile.id)}
        >
          <b>{index + 1}</b>{profile.name}
        </button>
      ))}
    </div>
  );
}

function MemberLine({ model }: { model: PanelModel }) {
  const style: ModelStyle = { "--model-color": safeColor(model.color) };
  return (
    <li className="ap-member" style={style}>
      <span className="ap-dot" aria-hidden="true" />
      <span className="ap-model">
        {model.label}
        {model.effort && <span className="ap-effort"> · {model.effort}</span>}
      </span>
      {model.billing && <span className="ap-meta">{billingLabel(model.billing)}</span>}
    </li>
  );
}

/** Members, providers and the estimated cost against the per-run limit. */
export function EnsemblePreviewView({ state }: { state: PreviewState | null }) {
  if (!state || state.status === "loading") {
    return (
      <div className="ap-thinking">
        <Loader2 className="ap-spin" size={16} aria-hidden="true" />
        Estimating the cost…
      </div>
    );
  }
  if (state.status === "error") {
    return (
      <div className="ap-failed">
        <strong>Ensemble unavailable</strong>
        <span>{state.error}</span>
      </div>
    );
  }
  const { preview } = state;
  const providers = [...new Set(preview.providers)].map(providerName);
  const free = preview.estimatedUsd <= 0 && !preview.unknownCost;
  return (
    <div className="ap-preview">
      <ul className="ap-members" aria-label="Members">
        {preview.members.map((model, index) => <MemberLine key={`${index}-${model.key}`} model={model} />)}
      </ul>
      {providers.length > 0 && (
        <p className="ap-preview-line">
          <span className="ap-preview-label">Sent to</span> {providers.join(", ")}
        </p>
      )}
      <p className="ap-preview-line" data-testid="ensemble-cost">
        <span className="ap-preview-label">Estimated API cost</span>{" "}
        {free ? "none (plan or local models)" : `≈ ${formatUsd(preview.estimatedUsd)}`}
        <span className="ap-preview-limit"> · limit {formatUsd(preview.limitUsd)} per run</span>
      </p>
      {preview.unknownCost && (
        <p className="ap-preview-note">Some models have no known price, so the estimate leaves them out.</p>
      )}
      {preview.blocked && (
        <div className="ap-blocked" role="alert">
          <strong>Over the cost limit</strong>
          <span>
            This run is estimated at {formatUsd(preview.estimatedUsd)}, above the per-run limit of {formatUsd(preview.limitUsd)}.
            Choose another profile, select less text, or raise the limit under Models, Text models, Ensemble profiles.
          </span>
        </div>
      )}
    </div>
  );
}

function phaseText(phase: PanelPhase, result: PanelResult | undefined) {
  switch (phase) {
    case "thinking":
      return "Thinking…";
    case "streaming":
      return "Writing…";
    case "done":
      return canApply(result) ? formatPanelDuration(result?.durationMs ?? null) ?? "Done" : "Empty";
    case "failed":
      return "Failed";
    case "cancelled":
      return "Cancelled";
  }
}

function CardBody({ phase, result, diff, showDiff, number }: {
  phase: PanelPhase;
  result: PanelResult | undefined;
  diff: DiffPart[] | null;
  showDiff: boolean;
  number: number;
}) {
  if (phase === "failed") {
    return (
      <div className="ap-failed">
        <strong>Failed</strong>
        <span>{result?.error || "Request failed"}</span>
      </div>
    );
  }
  if (phase === "cancelled") return <p className="ap-placeholder">Cancelled.</p>;
  const text = result?.text ?? "";
  if (!text) {
    if (phase === "done") return <p className="ap-placeholder">The model returned no text.</p>;
    return (
      <div className="ap-thinking">
        <Loader2 className="ap-spin" size={14} aria-hidden="true" />
        Waiting for the answer…
      </div>
    );
  }
  return (
    <ResultText
      text={text}
      diff={diff}
      showDiff={showDiff}
      streaming={phase === "streaming"}
      testId={`ensemble-text-${number}`}
      diffTestId={`ensemble-diff-${number}`}
    />
  );
}

type CardsProps = {
  members: readonly PanelModel[];
  count: number;
  results: Record<number, PanelResult>;
  selected: number;
  showDiff: boolean;
  /** Word diff of the selected card (null when off, not ready or too long). */
  diff: DiffPart[] | null;
  disabled: boolean;
  onSelect: (index: number) => void;
};

/** One card per member, numbered 1–n; the selected card has an outline. */
export function EnsembleCards({ members, count, results, selected, showDiff, diff, disabled, onSelect }: CardsProps) {
  const ids = useId();
  const listRef = useRef<HTMLDivElement>(null);

  // Keep the selected card visible when number keys select one further down.
  useEffect(() => {
    const card = listRef.current?.querySelector<HTMLElement>(`[data-card="${selected}"]`);
    card?.scrollIntoView?.({ block: "nearest" });
  }, [selected]);

  const columns = count <= 3 ? Math.max(1, count) : count === 4 ? 2 : 3;
  const gridStyle: ModelStyle = { "--ap-cols": String(columns) };
  return (
    <div ref={listRef} className="ap-cards" role="group" aria-label="Ensemble results" style={gridStyle}>
      {Array.from({ length: count }, (_, index) => {
        const result = results[index];
        const model = members[index];
        const phase: PanelPhase = result?.phase ?? "thinking";
        const label = result?.label || model?.label || "Model";
        const effort = (result ? result.effort : null) ?? model?.effort ?? null;
        const billing = billingLabel(result?.billing || model?.billing);
        const cost = phase === "done" ? formatPanelCost(result?.costUsd ?? null) : null;
        const isSelected = index === selected;
        const warnings = result && (phase === "done" || phase === "streaming") ? friendlyWarnings(result.warnings) : [];
        const style: ModelStyle = { "--model-color": safeColor(result?.color || model?.color) };
        const status = phaseText(phase, result);
        const headId = `${ids}-card-${index}`;
        return (
          <article
            key={index}
            className={isSelected ? "ap-ens-card on" : "ap-ens-card"}
            style={style}
            data-card={index}
            data-phase={phase}
            data-testid={`ensemble-card-${index + 1}`}
            aria-labelledby={headId}
            aria-busy={phase === "thinking" || phase === "streaming"}
          >
            <button
              id={headId}
              type="button"
              className="ap-ens-head"
              aria-pressed={isSelected}
              aria-keyshortcuts={String(index + 1)}
              aria-label={`${index + 1}. ${label}${effort ? ` · ${effort}` : ""}, ${billing ? `${billing}, ` : ""}${status}`}
              disabled={disabled}
              onClick={() => onSelect(index)}
            >
              <span className="ap-ens-num" aria-hidden="true">{index + 1}</span>
              <span className="ap-dot" aria-hidden="true" />
              <span className="ap-model" aria-hidden="true">
                {label}
                {effort && <span className="ap-effort"> · {effort}</span>}
              </span>
            </button>
            <div className="ap-ens-meta" aria-hidden="true">
              {(billing || cost) && <span className="ap-meta">{[billing, cost].filter(Boolean).join(" · ")}</span>}
              <span className={`ap-ens-phase ap-ens-phase-${phase}`}>{status}</span>
            </div>
            <div className="ap-ens-body" role="region" aria-label={`Result ${index + 1}`} tabIndex={0}>
              <CardBody
                phase={phase}
                result={result}
                diff={isSelected ? diff : null}
                showDiff={isSelected && showDiff}
                number={index + 1}
              />
            </div>
            <WarningList warnings={warnings} label={`Warnings for result ${index + 1}`} />
          </article>
        );
      })}
    </div>
  );
}
