import { useEffect, useState, type CSSProperties } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  HALO_EVENT,
  HIDDEN_HALO,
  RING_CIRCUMFERENCE,
  RING_RADIUS,
  chipColors,
  ensembleSegments,
  fadeDurationMs,
  normalizeHaloPayload,
  progressDashOffset,
  ringVisual,
  type HaloLayout,
} from "../lib/halo";
import "./halo.css";

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";
/** Length of the visible arc of the indeterminate spinner. */
const SPINNER_ARC = 20;

type HaloStyle = CSSProperties & Record<`--${string}`, string>;

/**
 * Content of the transparent, click-through `assist-halo` window: selection outline,
 * progress ring and label chip for a Text Assist job. It has no interactive controls.
 */
export function HaloView() {
  const [halo, setHalo] = useState<HaloLayout>(HIDDEN_HALO);
  const [fadedKey, setFadedKey] = useState<string | null>(null);
  const reducedMotion = usePrefersReducedMotion();

  useEffect(() => {
    const pending = listen<unknown>(HALO_EVENT, (event) => {
      setHalo(normalizeHaloPayload(event.payload, window.devicePixelRatio));
    });
    return () => {
      pending.then((unlisten) => unlisten()).catch(console.error);
    };
  }, []);

  const key = `${halo.jobId}:${halo.phase}`;
  const fadeMs = fadeDurationMs(halo.phase);
  useEffect(() => {
    if (fadeMs === null) return;
    const timer = window.setTimeout(() => setFadedKey(key), fadeMs);
    return () => window.clearTimeout(timer);
  }, [key, fadeMs]);

  const visible = halo.phase !== "hidden";
  const faded = visible && fadeMs !== null && fadedKey === key;
  const ensemble = halo.kind === "ensemble" && halo.colors.length > 1;
  const style: HaloStyle = {
    "--halo-color": halo.color,
    "--halo-gradient": ensemble ? `linear-gradient(90deg, ${halo.colors.join(", ")})` : halo.color,
  };
  const classes = [
    "halo-root",
    `halo-phase-${halo.phase}`,
    reducedMotion ? "halo-reduced-motion" : "",
    faded ? "halo-faded" : "",
  ].filter(Boolean).join(" ");

  return (
    <main className={classes} role="status" aria-live="polite" style={style}>
      {visible && <span className="halo-sr-only">{halo.label}</span>}
      {visible && (
        <div className="halo-layer" aria-hidden="true">
          {halo.box && <Outline halo={halo} ensemble={ensemble} />}
          <Badge halo={halo} ensemble={ensemble} />
        </div>
      )}
    </main>
  );
}

function Outline({ halo, ensemble }: { halo: HaloLayout; ensemble: boolean }) {
  const box = halo.box;
  if (!box) return null;
  const weight = halo.phase === "hold_cue" || halo.kind === "rewrite" ? "thick" : "thin";
  const classes = [
    "halo-outline",
    ensemble ? "halo-outline-gradient" : `halo-outline-${weight}`,
    halo.phase === "done" ? "halo-outline-done" : "",
  ].filter(Boolean).join(" ");
  return (
    <div
      className={classes}
      data-testid="halo-outline"
      style={{ left: `${box.x}px`, top: `${box.y}px`, width: `${box.width}px`, height: `${box.height}px` }}
    />
  );
}

function Badge({ halo, ensemble }: { halo: HaloLayout; ensemble: boolean }) {
  const chip = chipColors(halo);
  // The ring (30 px) is centered on the anchor point; the chip extends away from it.
  const position: CSSProperties = halo.badgeSide === "left"
    ? { left: `${halo.point.x + 15}px`, top: `${halo.point.y - 15}px`, transform: "translateX(-100%)" }
    : { left: `${halo.point.x - 15}px`, top: `${halo.point.y - 15}px` };
  return (
    <div className={`halo-badge halo-badge-${halo.badgeSide}`} data-testid="halo-badge" style={position}>
      {halo.label && (
        <span className="halo-chip" data-testid="halo-chip" style={{ background: chip.background, color: chip.color }}>
          {halo.label}
        </span>
      )}
      <Ring halo={halo} ensemble={ensemble} />
    </div>
  );
}

function Ring({ halo, ensemble }: { halo: HaloLayout; ensemble: boolean }) {
  const visual = ringVisual(halo);
  const center = 18;
  const segments = ensemble && (visual.type === "spinner" || visual.type === "progress") ? ensembleSegments(halo.colors) : [];

  return (
    <svg className="halo-ring" viewBox="0 0 36 36" data-testid="halo-ring" data-ring={visual.type}>
      <circle
        className="halo-ring-bg"
        cx={center}
        cy={center}
        r={17}
        fill={visual.type === "glyph" ? visual.fill : "#ffffff"}
      />
      {segments.length > 0 ? (
        <g className={visual.type === "spinner" ? "halo-spin" : undefined} data-testid="halo-segments">
          {segments.map((segment, index) => (
            <circle
              key={index}
              className="halo-ring-segment"
              cx={center}
              cy={center}
              r={RING_RADIUS}
              fill="none"
              stroke={segment.color}
              strokeWidth={4}
              strokeDasharray={segment.dashArray}
              transform={`rotate(${segment.rotation} ${center} ${center})`}
            />
          ))}
        </g>
      ) : (
        <>
          {(visual.type === "spinner" || visual.type === "progress") && (
            <circle className="halo-ring-track" cx={center} cy={center} r={RING_RADIUS} fill="none" stroke="#e6e6e6" strokeWidth={4} />
          )}
          {visual.type === "spinner" && (
            <g className="halo-spin">
              <circle
                className="halo-ring-bar"
                data-testid="halo-ring-bar"
                cx={center}
                cy={center}
                r={RING_RADIUS}
                fill="none"
                stroke={halo.color}
                strokeWidth={4}
                strokeLinecap="round"
                strokeDasharray={`${SPINNER_ARC} ${(RING_CIRCUMFERENCE - SPINNER_ARC).toFixed(2)}`}
                transform={`rotate(-90 ${center} ${center})`}
              />
            </g>
          )}
          {visual.type === "progress" && (
            <circle
              className="halo-ring-bar"
              data-testid="halo-ring-bar"
              data-progress={visual.progress}
              cx={center}
              cy={center}
              r={RING_RADIUS}
              fill="none"
              stroke={halo.color}
              strokeWidth={4}
              strokeLinecap={visual.progress > 0 ? "round" : "butt"}
              strokeDasharray={RING_CIRCUMFERENCE.toFixed(2)}
              strokeDashoffset={progressDashOffset(visual.progress).toFixed(2)}
              transform={`rotate(-90 ${center} ${center})`}
            />
          )}
          {visual.type === "plain" && (
            <circle cx={center} cy={center} r={RING_RADIUS} fill="none" stroke={visual.stroke} strokeWidth={4} />
          )}
        </>
      )}
      {visual.type === "glyph" && (
        <>
          {visual.ring && <circle cx={center} cy={center} r={RING_RADIUS + 1} fill="none" stroke={visual.ring} strokeWidth={3} />}
          <text className="halo-ring-glyph" data-testid="halo-ring-glyph" x={center} y={18.5} fill={visual.text}>
            {visual.glyph}
          </text>
        </>
      )}
    </svg>
  );
}

function usePrefersReducedMotion(): boolean {
  const [reduced, setReduced] = useState(() => matchesReducedMotion());
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const query = window.matchMedia(REDUCED_MOTION_QUERY);
    const onChange = () => setReduced(query.matches);
    query.addEventListener?.("change", onChange);
    return () => query.removeEventListener?.("change", onChange);
  }, []);
  return reduced;
}

function matchesReducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia(REDUCED_MOTION_QUERY).matches;
}
