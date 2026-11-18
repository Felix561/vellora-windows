// Types and pure helpers for the Text Assist halo window (`assist-halo`).
// The backend lays the halo out in physical pixels; the view works in CSS pixels.

export const HALO_EVENT = "text-assist-halo";
export const HALO_WINDOW_LABEL = "assist-halo";

export const HALO_PHASES = [
  "hidden",
  "hold_cue",
  "capturing",
  "thinking",
  "streaming",
  "checking",
  "replacing",
  "done",
  "no_change",
  "kept",
  "failed",
  "cancelled",
] as const;

export type HaloPhase = (typeof HALO_PHASES)[number];
export type HaloKind = "polish" | "rewrite" | "ensemble";
export type HaloMode = "box" | "point";
export type HaloRect = { x: number; y: number; width: number; height: number };
export type HaloPoint = { x: number; y: number };

export type HaloPayload = {
  jobId: string;
  phase: HaloPhase;
  /** Outline style: polish thin (2px), rewrite thick (3px), ensemble gradient of `colors`. */
  kind: HaloKind;
  /** box = draw the selection outline; point = badge only (caret/pointer anchor). */
  mode: HaloMode;
  /** "#rrggbb", model color. */
  color: string;
  /** Ensemble member colors (segmented ring + gradient outline). */
  colors?: string[];
  /** Chip text, e.g. "GPT-6 Luna · polish". */
  label: string;
  /** 0..1 while streaming; null = indeterminate. */
  progress: number | null;
  /** Window-relative physical px. */
  box: HaloRect | null;
  /** Window-relative physical px: box top-right (box mode) or caret top-right (point mode). */
  point: HaloPoint;
  /** Chip extends left of the ring (box mode default) or right (point mode). */
  badgeSide: "left" | "right";
  /** Device scale used by the backend layout. */
  scale: number;
};

/** A validated payload with geometry converted to CSS pixels. */
export type HaloLayout = {
  jobId: string;
  phase: HaloPhase;
  kind: HaloKind;
  mode: HaloMode;
  color: string;
  /** Valid ensemble colors; empty unless there are at least two. */
  colors: string[];
  label: string;
  progress: number | null;
  box: HaloRect | null;
  point: HaloPoint;
  badgeSide: "left" | "right";
};

export const HALO_FALLBACK_COLOR = "#8a8a8a";
export const HALO_KEPT_COLOR = "#c98a00";
export const HALO_FAILED_COLOR = "#d64545";

/** Radius of the progress arc in the 36x36 ring viewBox. */
export const RING_RADIUS = 13;
export const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;

const COLOR_PATTERN = /^#[0-9a-fA-F]{6}$/;
const SPINNING_PHASES: ReadonlySet<HaloPhase> = new Set(["hold_cue", "capturing", "thinking", "checking", "replacing"]);

export const HIDDEN_HALO: HaloLayout = {
  jobId: "",
  phase: "hidden",
  kind: "polish",
  mode: "point",
  color: HALO_FALLBACK_COLOR,
  colors: [],
  label: "",
  progress: null,
  box: null,
  point: { x: 0, y: 0 },
  badgeSide: "left",
};

export function isHaloPhase(value: unknown): value is HaloPhase {
  return typeof value === "string" && (HALO_PHASES as readonly string[]).includes(value);
}

export function isValidHaloColor(value: unknown): value is string {
  return typeof value === "string" && COLOR_PATTERN.test(value);
}

export function normalizeHaloColor(value: unknown): string {
  return isValidHaloColor(value) ? value.toLowerCase() : HALO_FALLBACK_COLOR;
}

/** Clamps progress to 0..1; anything that is not a finite number is indeterminate (null). */
export function clampProgress(value: unknown): number | null {
  if (typeof value !== "number" || !Number.isFinite(value)) return null;
  return Math.min(1, Math.max(0, value));
}

/**
 * Physical-to-CSS ratio: the backend scale of the target monitor when usable, else
 * devicePixelRatio, else 1. The backend moves the halo onto the target monitor right before it
 * sends the payload, so devicePixelRatio may still describe the previous monitor then.
 */
export function resolvePixelRatio(devicePixelRatio: unknown, scale: unknown): number {
  if (typeof scale === "number" && Number.isFinite(scale) && scale > 0) return scale;
  if (typeof devicePixelRatio === "number" && Number.isFinite(devicePixelRatio) && devicePixelRatio > 0) {
    return devicePixelRatio;
  }
  return 1;
}

export function physicalToCss(value: number, ratio: number): number {
  return value / ratio;
}

function finite(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function toRect(raw: unknown, ratio: number): HaloRect | null {
  if (!raw || typeof raw !== "object") return null;
  const { x, y, width, height } = raw as Record<string, unknown>;
  if (!finite(x) || !finite(y) || !finite(width) || !finite(height) || width <= 0 || height <= 0) return null;
  return {
    x: physicalToCss(x, ratio),
    y: physicalToCss(y, ratio),
    width: physicalToCss(width, ratio),
    height: physicalToCss(height, ratio),
  };
}

function toPoint(raw: unknown, ratio: number): HaloPoint | null {
  if (!raw || typeof raw !== "object") return null;
  const { x, y } = raw as Record<string, unknown>;
  if (!finite(x) || !finite(y)) return null;
  return { x: physicalToCss(x, ratio), y: physicalToCss(y, ratio) };
}

/**
 * Validates an untrusted `text-assist-halo` payload and converts its geometry to CSS px.
 * Anything unusable (not an object, unknown phase, no anchor) becomes the hidden halo.
 */
export function normalizeHaloPayload(raw: unknown, devicePixelRatio?: number): HaloLayout {
  if (!raw || typeof raw !== "object") return HIDDEN_HALO;
  const payload = raw as Record<string, unknown>;
  if (!isHaloPhase(payload.phase) || payload.phase === "hidden") {
    return { ...HIDDEN_HALO, jobId: typeof payload.jobId === "string" ? payload.jobId : "" };
  }
  const ratio = resolvePixelRatio(devicePixelRatio, payload.scale);
  const kind: HaloKind = payload.kind === "rewrite" || payload.kind === "ensemble" ? payload.kind : "polish";
  const box = payload.mode === "point" ? null : toRect(payload.box, ratio);
  const mode: HaloMode = box ? "box" : "point";
  const point = toPoint(payload.point, ratio) ?? (box ? { x: box.x + box.width, y: box.y } : null);
  if (!point) return HIDDEN_HALO;
  const colors = Array.isArray(payload.colors) ? payload.colors.filter(isValidHaloColor).map((c) => c.toLowerCase()) : [];
  const badgeSide = payload.badgeSide === "right" || payload.badgeSide === "left"
    ? payload.badgeSide
    : mode === "box" ? "left" : "right";
  return {
    jobId: typeof payload.jobId === "string" ? payload.jobId : "",
    phase: payload.phase,
    kind,
    mode,
    color: normalizeHaloColor(payload.color),
    colors: colors.length > 1 ? colors : [],
    label: typeof payload.label === "string" ? payload.label : "",
    progress: clampProgress(payload.progress),
    box,
    point,
    badgeSide,
  };
}

export type RingVisual =
  | { type: "spinner" }
  | { type: "progress"; progress: number }
  | { type: "glyph"; glyph: string; fill: string; ring?: string; text: string }
  | { type: "plain"; stroke: string };

/** How the 30 px ring looks in a given phase. */
export function ringVisual(layout: Pick<HaloLayout, "phase" | "progress" | "color">): RingVisual {
  const { phase, progress, color } = layout;
  if (SPINNING_PHASES.has(phase)) return { type: "spinner" };
  switch (phase) {
    case "streaming":
      return progress === null ? { type: "spinner" } : { type: "progress", progress };
    case "done":
      return { type: "glyph", glyph: "✓", fill: color, text: "#ffffff" };
    case "no_change":
      return { type: "glyph", glyph: "✓", fill: HALO_FALLBACK_COLOR, text: "#ffffff" };
    case "kept":
      return { type: "glyph", glyph: "…", fill: "#ffffff", ring: HALO_KEPT_COLOR, text: "#8a5d00" };
    case "failed":
      return { type: "glyph", glyph: "!", fill: HALO_FAILED_COLOR, text: "#ffffff" };
    case "cancelled":
    default:
      return { type: "plain", stroke: HALO_FALLBACK_COLOR };
  }
}

/** stroke-dashoffset that leaves `progress` (0..1) of the arc visible. */
export function progressDashOffset(progress: number): number {
  const clamped = Math.min(1, Math.max(0, progress));
  return RING_CIRCUMFERENCE * (1 - clamped);
}

export type RingSegment = { color: string; dashArray: string; rotation: number };

/** n equal ring segments with a small gap, starting at 12 o'clock. */
export function ensembleSegments(colors: string[], gap = 3): RingSegment[] {
  const n = colors.length;
  if (n === 0) return [];
  const length = Math.max(1, RING_CIRCUMFERENCE / n - (n > 1 ? gap : 0));
  return colors.map((color, index) => ({
    color,
    dashArray: `${length.toFixed(2)} ${(RING_CIRCUMFERENCE - length).toFixed(2)}`,
    rotation: -90 + (360 / n) * index,
  }));
}

/** Fade-out duration for terminal phases that clear themselves; null = stays until hidden. */
export function fadeDurationMs(phase: HaloPhase): number | null {
  if (phase === "done" || phase === "no_change") return 2000;
  if (phase === "cancelled") return 1500;
  return null;
}

/** Chip colors; light chips with dark text where the text was left untouched. */
export function chipColors(layout: Pick<HaloLayout, "phase" | "color" | "kind" | "colors">): { background: string; color: string } {
  switch (layout.phase) {
    case "kept":
      return { background: "#fff3d1", color: "#5c3d00" };
    case "failed":
      return { background: "#fde7e7", color: "#8a1f1f" };
    case "no_change":
      return { background: "#f1f1f1", color: "#2f2f2f" };
    case "cancelled":
      return { background: "#5f5f5f", color: "#ffffff" };
    default:
      if (layout.kind === "ensemble" && layout.colors.length > 1) return { background: "#3b3b3b", color: "#ffffff" };
      return { background: layout.color, color: "#ffffff" };
  }
}
