import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  PANEL_OPEN_EVENT,
  PANEL_RESULT_EVENT,
  billingLabel,
  formatPanelCost,
  formatPanelDuration,
  friendlyWarnings,
  nextModelIndex,
  normalizePanelInit,
  normalizePanelResult,
  presetKeyRange,
  rememberInstruction,
  targetAppName,
  type PanelInit,
  type PanelResult,
} from "../../src/lib/assistPanel";
import { AssistPanelView } from "../../src/views/AssistPanelView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
let initPayload: PanelInit | null;
let requestCounter: number;

function panelInit(overrides: Partial<PanelInit> = {}): PanelInit {
  return {
    sessionId: "s1",
    original: "the results shows that the method is robust",
    targetApp: "texstudio.exe",
    appearance: "notebook",
    presets: [
      { id: "polish", name: "Polish", instruction: "", builtin: true },
      { id: "clearer", name: "Clearer", instruction: "", builtin: true },
      { id: "academic", name: "Academic", instruction: "", builtin: true },
      { id: "professional", name: "Professional", instruction: "", builtin: true },
      { id: "custom", name: "Custom…", instruction: "", builtin: true },
    ],
    activePresetId: "polish",
    models: [
      { key: "openai/gpt-6-luna", label: "GPT-6 Luna", effort: "low", color: "#10a37f", billing: "api" },
      { key: "anthropic/claude-opus-5-5", label: "Claude Opus 5.5", effort: "medium", color: "#d97757", billing: "claude_plan" },
      { key: "google/gemini", label: "Gemini 3.8 Flash", effort: null, color: "#4285f4", billing: "api" },
    ],
    activeModelIndex: 0,
    customInstructions: ["Translate to English", "Make it shorter"],
    maxChars: 4000,
    ...overrides,
  };
}

function result(overrides: Partial<PanelResult> = {}): PanelResult {
  return {
    sessionId: "s1",
    requestId: "r1",
    presetId: "polish",
    modelIndex: 0,
    phase: "done",
    text: "The results show that the method is robust.",
    label: "GPT-6 Luna",
    effort: "low",
    billing: "api",
    color: "#10a37f",
    durationMs: 1400,
    costUsd: 0.0004,
    warnings: [],
    error: null,
    ...overrides,
  };
}

function runCalls() {
  return vi.mocked(invoke).mock.calls.filter(([command]) => command === "panel_run").map(([, args]) => args);
}

function lastRun() {
  const calls = runCalls();
  return calls[calls.length - 1];
}

function emit(event: string, payload: unknown) {
  act(() => handlers.get(event)?.({ payload }));
}

async function renderPanel() {
  const view = render(<AssistPanelView />);
  await waitFor(() => expect(runCalls().length).toBeGreaterThan(0));
  return view;
}

function press(key: string, init: KeyboardEventInit = {}) {
  fireEvent.keyDown(window, { key, ...init });
}

beforeEach(() => {
  handlers.clear();
  initPayload = panelInit();
  requestCounter = 0;
  vi.mocked(listen).mockImplementation(async (event, callback) => {
    handlers.set(event, callback as unknown as Handler);
    return () => undefined;
  });
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    switch (command) {
      case "get_panel_init":
        return initPayload;
      case "panel_run":
        requestCounter += 1;
        return `r${requestCounter}`;
      default:
        return undefined;
    }
  });
});

afterEach(() => {
  vi.mocked(invoke).mockReset();
  vi.mocked(listen).mockReset();
});

describe("AssistPanelView", () => {
  it("loads the init payload and runs the active preset and model", async () => {
    await renderPanel();
    expect(runCalls()).toEqual([{ presetId: "polish", modelIndex: 0, customInstruction: null, fresh: false }]);
    const chips = screen.getAllByRole("button", { pressed: true });
    expect(chips[0]).toHaveTextContent("1Polish");
    expect(screen.getByRole("button", { name: "5Custom…" })).toBeInTheDocument();
    expect(screen.getByTestId("panel-header")).toHaveTextContent("GPT-6 Luna · low");
    expect(screen.getByTestId("panel-header")).toHaveTextContent("→ texstudio");
    expect(screen.getByText("Thinking…")).toBeInTheDocument();
    expect(document.documentElement.dataset.appearance).toBe("notebook");
  });

  it("initializes from the open event and follows the classic appearance", async () => {
    initPayload = null;
    render(<AssistPanelView />);
    await waitFor(() => expect(handlers.has(PANEL_OPEN_EVENT)).toBe(true));
    expect(runCalls()).toHaveLength(0);
    emit(PANEL_OPEN_EVENT, panelInit({ appearance: "classic", activePresetId: "academic", activeModelIndex: 1 }));
    await waitFor(() => expect(runCalls()).toHaveLength(1));
    expect(lastRun()).toEqual({ presetId: "academic", modelIndex: 1, customInstruction: null, fresh: false });
    expect(document.documentElement.dataset.appearance).toBe("classic");
    // The same session again (event plus get_panel_init) does not start a second run.
    emit(PANEL_OPEN_EVENT, panelInit({ appearance: "classic" }));
    expect(runCalls()).toHaveLength(1);
  });

  it("streams text and ignores other sessions and stale requests", async () => {
    await renderPanel();
    emit(PANEL_RESULT_EVENT, result({ phase: "streaming", text: "The results" }));
    expect(screen.getByTestId("panel-text")).toHaveTextContent("The results");
    emit(PANEL_RESULT_EVENT, result({ sessionId: "old", phase: "streaming", text: "Other session" }));
    expect(screen.getByTestId("panel-text")).toHaveTextContent("The results");
    expect(screen.queryByText(/Other session/)).not.toBeInTheDocument();

    emit(PANEL_RESULT_EVENT, result());
    expect(screen.getByTestId("panel-text")).toHaveTextContent("The results show that the method is robust.");
    expect(screen.getByTestId("panel-header")).toHaveTextContent("API · ≈ $0.0004 · 1.4 s");

    // Another take replaces r1; late r1 events are ignored.
    press("r");
    await waitFor(() => expect(runCalls()).toHaveLength(2));
    expect(lastRun()).toMatchObject({ presetId: "polish", modelIndex: 0, fresh: true });
    expect(screen.getByText("Thinking…")).toBeInTheDocument();
    emit(PANEL_RESULT_EVENT, result({ text: "Stale text" }));
    expect(screen.queryByText("Stale text")).not.toBeInTheDocument();
    emit(PANEL_RESULT_EVENT, result({ requestId: "r2", text: "Second take." }));
    expect(screen.getByTestId("panel-text")).toHaveTextContent("Second take.");
  });

  it("preserves line breaks in the result", async () => {
    await renderPanel();
    emit(PANEL_RESULT_EVENT, result({ text: "Line one\nLine two" }));
    expect(screen.getByTestId("panel-text").textContent).toBe("Line one\nLine two");
  });

  it("switches presets with number keys", async () => {
    await renderPanel();
    press("3");
    await waitFor(() => expect(runCalls()).toHaveLength(2));
    expect(lastRun()).toEqual({ presetId: "academic", modelIndex: 0, customInstruction: null, fresh: false });
    expect(screen.getByRole("button", { name: "3Academic" })).toHaveAttribute("aria-pressed", "true");
    // Results are shown per preset + model.
    emit(PANEL_RESULT_EVENT, result({ requestId: "r2", presetId: "academic", text: "Academic text." }));
    expect(screen.getByTestId("panel-text")).toHaveTextContent("Academic text.");
    emit(PANEL_RESULT_EVENT, result({ requestId: "r1", presetId: "polish", text: "Polish text." }));
    expect(screen.getByTestId("panel-text")).toHaveTextContent("Academic text.");
    press("1");
    await waitFor(() => expect(runCalls()).toHaveLength(3));
    expect(screen.getByTestId("panel-text")).toHaveTextContent("Polish text.");
    // Selecting the current preset again and unknown numbers do nothing.
    press("1");
    press("9");
    expect(runCalls()).toHaveLength(3);
  });

  it("cycles models with Tab and Shift+Tab", async () => {
    await renderPanel();
    press("Tab");
    await waitFor(() => expect(runCalls()).toHaveLength(2));
    expect(lastRun()).toEqual({ presetId: "polish", modelIndex: 1, customInstruction: null, fresh: false });
    expect(screen.getByTestId("panel-header")).toHaveTextContent("Claude Opus 5.5 · medium");
    expect(screen.getByTestId("panel-header")).toHaveTextContent("Claude plan");
    press("Tab", { shiftKey: true });
    press("Tab", { shiftKey: true });
    await waitFor(() => expect(runCalls()).toHaveLength(4));
    expect(lastRun()).toMatchObject({ modelIndex: 2 });
    press("Tab");
    await waitFor(() => expect(runCalls()).toHaveLength(5));
    expect(lastRun()).toMatchObject({ modelIndex: 0 });
  });

  it("toggles a word diff with D", async () => {
    await renderPanel();
    emit(PANEL_RESULT_EVENT, result());
    press("d");
    const diff = screen.getByTestId("panel-diff");
    const removed = [...diff.querySelectorAll("del")].map((node) => node.textContent?.trim());
    const added = [...diff.querySelectorAll("ins")].map((node) => node.textContent?.trim());
    expect(removed).toContain("shows");
    expect(added).toContain("show");
    expect(added).toContain("robust.");
    expect(diff).toHaveTextContent("method is");
    expect(screen.getByRole("button", { name: "D diff" })).toHaveAttribute("aria-pressed", "true");
    press("D");
    expect(screen.queryByTestId("panel-diff")).not.toBeInTheDocument();
    expect(screen.getByTestId("panel-text")).toBeInTheDocument();
  });

  it("applies with Enter only when the result is done", async () => {
    await renderPanel();
    press("Enter");
    emit(PANEL_RESULT_EVENT, result({ phase: "streaming", text: "The results" }));
    press("Enter");
    expect(invoke).not.toHaveBeenCalledWith("panel_apply", expect.anything());
    emit(PANEL_RESULT_EVENT, result({ text: "  " }));
    press("Enter");
    expect(invoke).not.toHaveBeenCalledWith("panel_apply", expect.anything());
    emit(PANEL_RESULT_EVENT, result());
    press("Enter");
    expect(invoke).toHaveBeenCalledWith("panel_apply", { requestId: "r1" });
    // No second apply while the first is in flight.
    press("Enter");
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "panel_apply")).toHaveLength(1);
  });

  it("copies with Ctrl+C unless text is selected", async () => {
    await renderPanel();
    emit(PANEL_RESULT_EVENT, result());
    press("c", { ctrlKey: true });
    expect(invoke).toHaveBeenCalledWith("panel_copy", { requestId: "r1" });
    expect(await screen.findAllByText("Copied")).not.toHaveLength(0);

    vi.mocked(invoke).mockClear();
    const selection = window.getSelection();
    const range = document.createRange();
    range.selectNodeContents(screen.getByTestId("panel-text"));
    selection?.removeAllRanges();
    selection?.addRange(range);
    press("c", { ctrlKey: true });
    expect(invoke).not.toHaveBeenCalledWith("panel_copy", expect.anything());
    selection?.removeAllRanges();
  });

  it("closes with Esc and the close button", async () => {
    await renderPanel();
    press("Escape");
    expect(invoke).toHaveBeenCalledWith("panel_close");
    vi.mocked(invoke).mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(invoke).toHaveBeenCalledWith("panel_close");
  });

  it("runs the Custom preset with the typed instruction", async () => {
    await renderPanel();
    press("5");
    const input = screen.getByRole("textbox", { name: "Custom instruction" });
    await waitFor(() => expect(input).toHaveFocus());
    expect(runCalls()).toHaveLength(1);
    expect(screen.getByText("Type an instruction above and press Enter.")).toBeInTheDocument();

    // Keys typed in the input do not trigger panel shortcuts.
    fireEvent.keyDown(input, { key: "r" });
    fireEvent.keyDown(input, { key: "2" });
    expect(runCalls()).toHaveLength(1);

    // Up/down cycles the remembered instructions.
    fireEvent.keyDown(input, { key: "ArrowUp" });
    expect(input).toHaveValue("Translate to English");
    fireEvent.keyDown(input, { key: "ArrowUp" });
    expect(input).toHaveValue("Make it shorter");
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(input).toHaveValue("Translate to English");

    fireEvent.change(input, { target: { value: "Make it friendlier" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(runCalls()).toHaveLength(2));
    expect(lastRun()).toEqual({ presetId: "custom", modelIndex: 0, customInstruction: "Make it friendlier", fresh: true });

    emit(PANEL_RESULT_EVENT, result({ requestId: "r2", presetId: "custom", text: "Friendly text." }));
    expect(screen.getByTestId("panel-text")).toHaveTextContent("Friendly text.");

    // Switching the model reuses the instruction.
    press("Tab");
    await waitFor(() => expect(runCalls()).toHaveLength(3));
    expect(lastRun()).toEqual({ presetId: "custom", modelIndex: 1, customInstruction: "Make it friendlier", fresh: true });
    press("Tab", { shiftKey: true });
    await waitFor(() => expect(runCalls()).toHaveLength(4));
    expect(lastRun()).toEqual({ presetId: "custom", modelIndex: 0, customInstruction: "Make it friendlier", fresh: false });

    // Esc in the input still closes.
    fireEvent.keyDown(input, { key: "Escape" });
    expect(invoke).toHaveBeenCalledWith("panel_close");
  });

  it("starts a custom run at init with the newest remembered instruction", async () => {
    initPayload = panelInit({ activePresetId: "custom" });
    await renderPanel();
    expect(lastRun()).toEqual({ presetId: "custom", modelIndex: 0, customInstruction: "Translate to English", fresh: true });
  });

  it("shows guard warnings as friendly notes", async () => {
    await renderPanel();
    emit(PANEL_RESULT_EVENT, result({ warnings: ["guard_markup", "guard_preamble", "guard_wrapper", "guard_too_many_changes"] }));
    const list = screen.getByRole("list", { name: "Warnings" });
    expect(list).toHaveTextContent("LaTeX/Markdown/code may have changed");
    expect(list).toHaveTextContent("Large change");
    expect(screen.getAllByText("Model added extra text")).toHaveLength(1);
    // Warnings do not block Apply in the panel.
    press("Enter");
    expect(invoke).toHaveBeenCalledWith("panel_apply", { requestId: "r1" });
  });

  it("shows the error label when a run fails", async () => {
    await renderPanel();
    emit(PANEL_RESULT_EVENT, result({ phase: "failed", text: "", error: "Rate limited", durationMs: null }));
    expect(screen.getByText("Failed")).toBeInTheDocument();
    expect(screen.getAllByText(/Rate limited/).length).toBeGreaterThan(0);
    press("Enter");
    expect(invoke).not.toHaveBeenCalledWith("panel_apply", expect.anything());
  });

  it("shows a failed state when panel_run is rejected", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "get_panel_init") return initPayload;
      if (command === "panel_run") throw new Error("No API key");
      return undefined;
    });
    render(<AssistPanelView />);
    expect(await screen.findByText("No API key")).toBeInTheDocument();
  });

  it("explains that ensemble is not available yet", async () => {
    await renderPanel();
    press("e");
    expect(screen.getAllByText("Ensemble arrives in a later update").length).toBeGreaterThan(0);
  });
});

describe("assistPanel helpers", () => {
  it("formats labels", () => {
    expect(billingLabel("chatgpt_plan")).toBe("ChatGPT plan");
    expect(billingLabel("local")).toBe("Local");
    expect(billingLabel("other")).toBe("other");
    expect(formatPanelDuration(820)).toBe("820 ms");
    expect(formatPanelDuration(7900)).toBe("7.9 s");
    expect(formatPanelDuration(null)).toBeNull();
    expect(formatPanelCost(0.0004)).toBe("≈ $0.0004");
    expect(formatPanelCost(0.12)).toBe("≈ $0.12");
    expect(formatPanelCost(null)).toBeNull();
    expect(targetAppName("TeXstudio.EXE")).toBe("TeXstudio");
    expect(presetKeyRange(5)).toBe("1–5");
    expect(presetKeyRange(12)).toBe("1–9");
    expect(friendlyWarnings(["guard_length", "guard_line_count", "guard_incomplete", "x"])).toEqual([
      "Length changed a lot",
      "Line breaks changed",
      "Output incomplete",
      "Check this result",
    ]);
  });

  it("wraps model indexes", () => {
    expect(nextModelIndex(2, 3, 1)).toBe(0);
    expect(nextModelIndex(0, 3, -1)).toBe(2);
    expect(nextModelIndex(0, 0, 1)).toBe(-1);
  });

  it("remembers at most five custom instructions, newest first", () => {
    expect(rememberInstruction(["a", "b", "c", "d", "e"], " c ")).toEqual(["c", "a", "b", "d", "e"]);
    expect(rememberInstruction(["a", "b", "c", "d", "e"], "f")).toEqual(["f", "a", "b", "c", "d"]);
  });

  it("validates payloads", () => {
    expect(normalizePanelInit(null)).toBeNull();
    expect(normalizePanelInit({ sessionId: "" })).toBeNull();
    const init = normalizePanelInit({ ...panelInit(), activePresetId: "missing", activeModelIndex: 7, appearance: "x" });
    expect(init).toMatchObject({ activePresetId: "polish", activeModelIndex: 2, appearance: "notebook" });
    const many = Array.from({ length: 12 }, (_, i) => ({ id: `p${i}`, name: `P${i}`, instruction: "", builtin: false }));
    expect(normalizePanelInit({ ...panelInit(), presets: many })?.presets).toHaveLength(9);
    expect(normalizePanelResult({ ...result(), phase: "weird" })).toBeNull();
    expect(normalizePanelResult({ ...result(), color: "red" })?.color).toBe("#8a8a8a");
  });
});
