import { act, render, screen } from "@testing-library/react";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  HALO_EVENT,
  HALO_FALLBACK_COLOR,
  HIDDEN_HALO,
  RING_CIRCUMFERENCE,
  chipColors,
  clampProgress,
  ensembleSegments,
  fadeDurationMs,
  normalizeHaloColor,
  normalizeHaloPayload,
  progressDashOffset,
  resolvePixelRatio,
  ringVisual,
  type HaloPayload,
} from "../../src/lib/halo";
import { HaloView } from "../../src/views/HaloView";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

type Handler = (event: { payload: unknown }) => void;
let handler: Handler | null;
const unlisten = vi.fn();
const originalRatio = window.devicePixelRatio;
const originalMatchMedia = window.matchMedia;

function payload(overrides: Partial<HaloPayload> = {}): HaloPayload {
  return {
    jobId: "job-1",
    phase: "thinking",
    kind: "polish",
    mode: "box",
    color: "#10a37f",
    label: "GPT-6 Luna · polish",
    progress: null,
    box: { x: 150, y: 300, width: 600, height: 90 },
    point: { x: 750, y: 300 },
    badgeSide: "left",
    scale: 1.5,
    ...overrides,
  };
}

function setPixelRatio(value: number | undefined) {
  Object.defineProperty(window, "devicePixelRatio", { configurable: true, writable: true, value });
}

function mockReducedMotion(matches: boolean) {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: matches && query.includes("prefers-reduced-motion"),
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
  });
}

async function renderHalo() {
  const view = render(<HaloView />);
  await vi.waitFor(() => expect(handler).not.toBeNull());
  return view;
}

function emit(next: unknown) {
  act(() => handler?.({ payload: next }));
}

beforeEach(() => {
  handler = null;
  setPixelRatio(1.5);
  mockReducedMotion(false);
  vi.mocked(listen).mockImplementation(async (event, callback) => {
    expect(event).toBe(HALO_EVENT);
    handler = callback as unknown as Handler;
    return unlisten;
  });
});

afterEach(() => {
  setPixelRatio(originalRatio);
  Object.defineProperty(window, "matchMedia", { configurable: true, writable: true, value: originalMatchMedia });
});

describe("halo helpers", () => {
  it("maps unknown or hidden phases and garbage payloads to the hidden halo", () => {
    expect(normalizeHaloPayload(null)).toBe(HIDDEN_HALO);
    expect(normalizeHaloPayload("thinking")).toBe(HIDDEN_HALO);
    expect(normalizeHaloPayload({ ...payload(), phase: "exploding" }).phase).toBe("hidden");
    expect(normalizeHaloPayload(payload({ phase: "hidden" })).phase).toBe("hidden");
  });

  it("clamps progress and treats non-numbers as indeterminate", () => {
    expect(clampProgress(-0.2)).toBe(0);
    expect(clampProgress(0.42)).toBe(0.42);
    expect(clampProgress(3)).toBe(1);
    expect(clampProgress(null)).toBeNull();
    expect(clampProgress(Number.NaN)).toBeNull();
    expect(clampProgress("0.5")).toBeNull();
  });

  it("converts physical to CSS px with the target monitor's scale, falling back to devicePixelRatio", () => {
    // devicePixelRatio may still be the previous monitor's right after the window moved.
    expect(resolvePixelRatio(2, 1.25)).toBe(1.25);
    expect(resolvePixelRatio(0, 1.25)).toBe(1.25);
    expect(resolvePixelRatio(undefined, 1.25)).toBe(1.25);
    expect(resolvePixelRatio(1.5, 0)).toBe(1.5);
    expect(resolvePixelRatio(1.5, undefined)).toBe(1.5);
    expect(resolvePixelRatio(undefined, 0)).toBe(1);
    const layout = normalizeHaloPayload(payload(), 1.5);
    expect(layout.box).toEqual({ x: 100, y: 200, width: 400, height: 60 });
    expect(layout.point).toEqual({ x: 500, y: 200 });
    expect(normalizeHaloPayload(payload({ scale: 2 }), 0).box).toEqual({ x: 75, y: 150, width: 300, height: 45 });
  });

  it("validates colors with a gray fallback and keeps only valid ensemble colors", () => {
    expect(normalizeHaloColor("#D97757")).toBe("#d97757");
    expect(normalizeHaloColor("red")).toBe(HALO_FALLBACK_COLOR);
    expect(normalizeHaloColor("#fff")).toBe(HALO_FALLBACK_COLOR);
    expect(normalizeHaloColor("#10a37f; background: url(x)")).toBe(HALO_FALLBACK_COLOR);
    const layout = normalizeHaloPayload(payload({ kind: "ensemble", colors: ["#10a37f", "nope", "#d97757"] }), 1);
    expect(layout.colors).toEqual(["#10a37f", "#d97757"]);
    expect(normalizeHaloPayload(payload({ colors: ["#10a37f"] }), 1).colors).toEqual([]);
  });

  it("falls back to point mode without a usable box and derives the anchor from a box", () => {
    const noBox = normalizeHaloPayload(payload({ box: null, scale: 1 }), 1);
    expect(noBox.mode).toBe("point");
    expect(noBox.box).toBeNull();
    const derived = normalizeHaloPayload({ ...payload({ scale: 1 }), point: undefined }, 1);
    expect(derived.point).toEqual({ x: 750, y: 300 });
    expect(normalizeHaloPayload({ ...payload({ scale: 1 }), box: null, point: undefined }, 1).phase).toBe("hidden");
  });

  it("describes ring, chip and fade per phase", () => {
    expect(ringVisual({ phase: "thinking", progress: null, color: "#10a37f" }).type).toBe("spinner");
    expect(ringVisual({ phase: "streaming", progress: null, color: "#10a37f" }).type).toBe("spinner");
    expect(ringVisual({ phase: "streaming", progress: 0.5, color: "#10a37f" })).toEqual({ type: "progress", progress: 0.5 });
    expect(ringVisual({ phase: "failed", progress: null, color: "#10a37f" })).toMatchObject({ glyph: "!", fill: "#d64545" });
    expect(progressDashOffset(0)).toBeCloseTo(RING_CIRCUMFERENCE);
    expect(progressDashOffset(1)).toBe(0);
    expect(fadeDurationMs("done")).toBe(2000);
    expect(fadeDurationMs("cancelled")).toBe(1500);
    expect(fadeDurationMs("failed")).toBeNull();
    expect(chipColors({ phase: "failed", color: "#10a37f", kind: "polish", colors: [] }).color).not.toBe("#ffffff");
    expect(ensembleSegments(["#10a37f", "#d97757", "#4285f4"]).map((s) => s.rotation)).toEqual([-90, 30, 150]);
  });
});

describe("HaloView", () => {
  it("renders nothing visible while hidden and unsubscribes on unmount", async () => {
    const view = await renderHalo();
    const root = screen.getByRole("status");
    expect(root).toBeEmptyDOMElement();
    emit(payload());
    expect(root).not.toBeEmptyDOMElement();
    emit(payload({ phase: "hidden" }));
    expect(root).toBeEmptyDOMElement();
    view.unmount();
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalled());
  });

  it("never draws an outline; the ring sits at the selection's top-right in CSS px", async () => {
    await renderHalo();
    emit(payload());
    expect(screen.queryByTestId("halo-outline")).not.toBeInTheDocument();
    // Ring centered on the selection's top-right corner (physical px / devicePixelRatio), chip to its left.
    expect(screen.getByTestId("halo-badge")).toHaveStyle({ left: "515px", top: "185px" });
    expect(screen.getByTestId("halo-badge")).toHaveClass("halo-badge-left");
    emit(payload({ kind: "rewrite" }));
    expect(screen.queryByTestId("halo-outline")).not.toBeInTheDocument();
  });

  it("shows a spinning ring for the hold cue", async () => {
    await renderHalo();
    emit(payload({ phase: "hold_cue", label: "Rewrite" }));
    expect(screen.getByTestId("halo-ring")).toHaveAttribute("data-ring", "spinner");
    expect(screen.getByTestId("halo-chip")).toHaveTextContent("Rewrite");
  });

  it("fills the arc to the streaming progress", async () => {
    await renderHalo();
    emit(payload({ phase: "streaming", progress: 0.25 }));
    const bar = screen.getByTestId("halo-ring-bar");
    expect(bar).toHaveAttribute("data-progress", "0.25");
    expect(Number(bar.getAttribute("stroke-dashoffset"))).toBeCloseTo(RING_CIRCUMFERENCE * 0.75, 1);
    emit(payload({ phase: "streaming", progress: 7 }));
    expect(Number(screen.getByTestId("halo-ring-bar").getAttribute("stroke-dashoffset"))).toBe(0);
  });

  it("shows ✓ when done and fades out after about two seconds", async () => {
    await renderHalo();
    vi.useFakeTimers();
    emit(payload({ phase: "done", label: "5 fixes" }));
    expect(screen.getByTestId("halo-ring-glyph")).toHaveTextContent("✓");
    expect(screen.queryByTestId("halo-outline")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("5 fixes");
    expect(screen.getByRole("status")).not.toHaveClass("halo-faded");
    act(() => vi.advanceTimersByTime(2000));
    expect(screen.getByRole("status")).toHaveClass("halo-faded");
  });

  it("shows ! and the label when failed and stays until hidden", async () => {
    await renderHalo();
    vi.useFakeTimers();
    emit(payload({ phase: "failed", label: "Select some text first" }));
    expect(screen.getByTestId("halo-ring-glyph")).toHaveTextContent("!");
    expect(screen.getByTestId("halo-chip")).toHaveTextContent("Select some text first");
    expect(screen.getByRole("status")).toHaveAttribute("aria-live", "polite");
    expect(screen.getByRole("status")).toHaveTextContent("Select some text first");
    act(() => vi.advanceTimersByTime(10_000));
    expect(screen.getByRole("status")).not.toHaveClass("halo-faded");
  });

  it("draws only the badge in point mode", async () => {
    await renderHalo();
    emit(payload({ mode: "point", box: null, point: { x: 300, y: 450 }, badgeSide: "right" }));
    expect(screen.queryByTestId("halo-outline")).not.toBeInTheDocument();
    const badge = screen.getByTestId("halo-badge");
    expect(badge).toHaveClass("halo-badge-right");
    expect(badge).toHaveStyle({ left: "185px", top: "285px" });
  });

  it("draws one ring segment per ensemble member", async () => {
    await renderHalo();
    emit(payload({ kind: "ensemble", colors: ["#10a37f", "#d97757", "#4285f4"], label: "Ensemble · 2 / 3", progress: 0.6, phase: "streaming" }));
    const segments = screen.getByTestId("halo-segments").querySelectorAll("circle");
    expect(segments).toHaveLength(3);
    expect(Array.from(segments).map((s) => s.getAttribute("stroke"))).toEqual(["#10a37f", "#d97757", "#4285f4"]);
    expect(screen.queryByTestId("halo-outline")).not.toBeInTheDocument();
  });

  it("marks the root for reduced motion", async () => {
    mockReducedMotion(true);
    await renderHalo();
    emit(payload());
    expect(screen.getByRole("status")).toHaveClass("halo-reduced-motion");
  });

  it("does not mark reduced motion by default", async () => {
    await renderHalo();
    emit(payload());
    expect(screen.getByRole("status")).not.toHaveClass("halo-reduced-motion");
  });
});
