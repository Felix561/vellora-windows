import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  PANEL_CHORD_EVENT,
  PANEL_OPEN_EVENT,
  PANEL_RESULT_EVENT,
  billingLabel,
  formatPanelCost,
  formatPanelDuration,
  friendlyWarnings,
  nextModelIndex,
  isRunningPhase,
  normalizePanelChord,
  normalizePanelInit,
  normalizePanelResult,
  presetKeyRange,
  rememberInstruction,
  shortModelLabel,
  targetAppName,
  formatUsd,
  normalizeEnsemblePreview,
  type EnsemblePreview,
  type PanelInit,
  type PanelModel,
  type PanelResult,
} from "../../src/lib/assistPanel";
import { ensembleGate, isEnsembleRunning, type EnsembleRun, type PreviewState } from "../../src/lib/assistEnsemble";
import { AssistPanelView } from "../../src/views/AssistPanelView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
let initPayload: PanelInit | null;
let requestCounter: number;
let previews: Record<string, EnsemblePreview>;
let runEnsemble: () => Promise<string>;

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
    ensembleProfiles: [
      { id: "trio", name: "Trio", members: [LUNA, OPUS, GEMINI] },
      { id: "duo", name: "Duo", members: [LUNA, { ...LUNA, key: "openai/gpt-6-luna/high", effort: "high" }] },
    ],
    ensembleMaxUsd: 0.05,
    ...overrides,
  };
}

const LUNA: PanelModel = { key: "openai/gpt-6-luna/low", label: "GPT-6 Luna", effort: "low", color: "#10a37f", billing: "api" };
const OPUS: PanelModel = { key: "claude_cli/claude-opus-5-5/medium", label: "Claude Opus 5.5", effort: "medium", color: "#d97757", billing: "claude_plan" };
const GEMINI: PanelModel = { key: "gemini/gemini/default", label: "Gemini 3.8 Flash", effort: null, color: "#4285f4", billing: "api" };

function ensemblePreview(overrides: Partial<EnsemblePreview> = {}): EnsemblePreview {
  return {
    profileId: "trio",
    members: [LUNA, OPUS, GEMINI],
    providers: ["openai", "claude_cli", "gemini"],
    estimatedUsd: 0.0123,
    unknownCost: false,
    limitUsd: 0.05,
    blocked: false,
    ...overrides,
  };
}

function member(index: number, overrides: Partial<PanelResult> = {}): PanelResult {
  const model = [LUNA, OPUS, GEMINI][index] ?? LUNA;
  return result({
    requestId: `m${index}`,
    modelIndex: 18446744073709552000,
    phase: "thinking",
    text: "",
    label: model.label,
    effort: model.effort,
    billing: model.billing,
    color: model.color,
    durationMs: null,
    costUsd: null,
    ensembleRunId: "e1",
    memberIndex: index,
    ...overrides,
  });
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
  await screen.findByTestId("panel-original");
  return view;
}

/** Shift+Win while the panel is open (the backend routes it to the panel). */
function chord(kind: "tap" | "hold" = "tap", sessionId = "s1") {
  emit(PANEL_CHORD_EVENT, { sessionId, kind });
}

/** Opens the panel and starts the current choice with a tap. */
async function startPanel() {
  const view = await renderPanel();
  chord();
  await waitFor(() => expect(runCalls()).toHaveLength(1));
  return view;
}

function press(key: string, init: KeyboardEventInit = {}) {
  fireEvent.keyDown(window, { key, ...init });
}

function startButton() {
  return screen.getByRole("button", { name: /^(Start|Run again|Running…)/ });
}

beforeEach(() => {
  handlers.clear();
  initPayload = panelInit();
  requestCounter = 0;
  previews = {
    trio: ensemblePreview(),
    duo: ensemblePreview({ profileId: "duo", members: [LUNA, { ...LUNA, effort: "high" }], providers: ["openai"], estimatedUsd: 0.002, unknownCost: true }),
  };
  runEnsemble = async () => "e1";
  vi.mocked(listen).mockImplementation(async (event, callback) => {
    handlers.set(event, callback as unknown as Handler);
    return () => undefined;
  });
  vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => {
    switch (command) {
      case "get_panel_init":
        return initPayload;
      case "panel_run":
        requestCounter += 1;
        return `r${requestCounter}`;
      case "panel_ensemble_preview":
        return previews[(args as { profileId: string }).profileId];
      case "panel_run_ensemble":
        return runEnsemble();
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
  it("opens in setup: shows the selection and choices but sends nothing", async () => {
    await renderPanel();
    expect(screen.getByTestId("panel-original")).toHaveTextContent("the results shows that the method is robust");
    const chips = screen.getAllByRole("button", { pressed: true });
    expect(chips[0]).toHaveTextContent("1Polish");
    expect(screen.getByRole("button", { name: "5Custom…" })).toHaveAttribute("aria-keyshortcuts", "5");
    const modelBar = screen.getByRole("toolbar", { name: "Model" });
    expect(within(modelBar).getAllByRole("button").map((button) => button.textContent)).toEqual([
      "GPT-6 Luna · low",
      "Claude Opus 5.5 · medium",
      "Gemini 3.8 Flash",
    ]);
    expect(within(modelBar).getByRole("button", { name: "GPT-6 Luna · low" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "EEnsemble" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByTestId("panel-header")).toHaveTextContent("GPT-6 Luna · low");
    expect(screen.getByTestId("panel-header")).toHaveTextContent("→ texstudio");
    expect(startButton()).toHaveAccessibleName("Start");
    expect(startButton()).toHaveAttribute("aria-keyshortcuts", "Shift+Meta");
    expect(screen.getByRole("status")).toHaveTextContent("Ready. Press Shift+Win or Start to run Polish with GPT-6 Luna");
    expect(document.documentElement.dataset.appearance).toBe("notebook");
    await act(async () => undefined);
    expect(runCalls()).toHaveLength(0);
    expect(invokeCalls("panel_ensemble_preview")).toHaveLength(0);
    expect(invokeCalls("panel_run_ensemble")).toHaveLength(0);
  });

  it("initializes from the open event and follows the classic appearance", async () => {
    initPayload = null;
    render(<AssistPanelView />);
    await waitFor(() => expect(handlers.has(PANEL_OPEN_EVENT)).toBe(true));
    emit(PANEL_OPEN_EVENT, panelInit({ appearance: "classic", activePresetId: "academic", activeModelIndex: 1 }));
    expect(await screen.findByTestId("panel-original")).toBeInTheDocument();
    expect(document.documentElement.dataset.appearance).toBe("classic");
    expect(screen.getByRole("button", { name: "3Academic" })).toHaveAttribute("aria-pressed", "true");
    expect(runCalls()).toHaveLength(0);
    chord();
    await waitFor(() => expect(runCalls()).toHaveLength(1));
    expect(lastRun()).toEqual({ presetId: "academic", modelIndex: 1, customInstruction: null, fresh: false });
    // The same session again (event plus get_panel_init) keeps the state.
    emit(PANEL_OPEN_EVENT, panelInit({ appearance: "classic" }));
    expect(runCalls()).toHaveLength(1);
    expect(screen.getByText("Thinking…")).toBeInTheDocument();
  });

  it("starts the current choice on a Shift+Win chord; a tap while it runs does nothing", async () => {
    await renderPanel();
    // Chords of another session are ignored.
    chord("tap", "old");
    await act(async () => undefined);
    expect(runCalls()).toHaveLength(0);
    chord("hold");
    await waitFor(() => expect(runCalls()).toHaveLength(1));
    expect(lastRun()).toEqual({ presetId: "polish", modelIndex: 0, customInstruction: null, fresh: false });
    expect(screen.getByText("Thinking…")).toBeInTheDocument();
    expect(startButton()).toHaveAccessibleName("Running…");
    expect(startButton()).toBeDisabled();

    chord();
    emit(PANEL_RESULT_EVENT, result({ phase: "streaming", text: "The results" }));
    chord();
    await act(async () => undefined);
    expect(runCalls()).toHaveLength(1);
    expect(screen.getAllByText(/Already running/).length).toBeGreaterThan(0);

    // After the result, the next tap is another take.
    emit(PANEL_RESULT_EVENT, result());
    expect(startButton()).toHaveAccessibleName("Run again");
    chord();
    await waitFor(() => expect(runCalls()).toHaveLength(2));
    expect(lastRun()).toEqual({ presetId: "polish", modelIndex: 0, customInstruction: null, fresh: true });
  });

  it("starts with the Start button", async () => {
    await renderPanel();
    fireEvent.click(startButton());
    await waitFor(() => expect(runCalls()).toHaveLength(1));
    expect(lastRun()).toEqual({ presetId: "polish", modelIndex: 0, customInstruction: null, fresh: false });
  });

  it("can be dragged by its header while the buttons stay clickable", async () => {
    await renderPanel();
    const region = screen.getByTestId("panel-drag-region");
    expect(region).toHaveAttribute("data-tauri-drag-region", "deep");
    expect(screen.getByTestId("panel-header")).toHaveAttribute("data-tauri-drag-region", "deep");
    // Buttons carry no drag attribute, so Tauri keeps them clickable.
    const close = within(region).getByRole("button", { name: "Close" });
    expect(close).not.toHaveAttribute("data-tauri-drag-region");
    expect(within(region).getByRole("button", { name: "1Polish" })).not.toHaveAttribute("data-tauri-drag-region");
    fireEvent.click(close);
    expect(invoke).toHaveBeenCalledWith("panel_close");
    // The ensemble setup has a drag region too.
    press("e");
    await screen.findByTestId("ensemble-cost");
    expect(screen.getByTestId("panel-drag-region")).toHaveAttribute("data-tauri-drag-region", "deep");
  });

  it("streams text and ignores other sessions and stale requests", async () => {
    await startPanel();
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

  it("does nothing on R before anything ran", async () => {
    await renderPanel();
    press("r");
    expect(screen.getByRole("button", { name: "R again" })).toBeDisabled();
    await act(async () => undefined);
    expect(runCalls()).toHaveLength(0);
  });

  it("preserves line breaks in the result", async () => {
    await startPanel();
    emit(PANEL_RESULT_EVENT, result({ text: "Line one\nLine two" }));
    expect(screen.getByTestId("panel-text").textContent).toBe("Line one\nLine two");
  });

  it("never runs on a preset or model change after a result; the next tap does", async () => {
    await startPanel();
    emit(PANEL_RESULT_EVENT, result({ text: "Polish text." }));

    press("3");
    expect(screen.getByRole("button", { name: "3Academic" })).toHaveAttribute("aria-pressed", "true");
    // Not run yet for Academic: the setup shows the selection again.
    expect(screen.getByTestId("panel-original")).toBeInTheDocument();
    press("Enter");
    expect(invokeCalls("panel_apply")).toHaveLength(0);
    await act(async () => undefined);
    expect(runCalls()).toHaveLength(1);

    chord();
    await waitFor(() => expect(runCalls()).toHaveLength(2));
    expect(lastRun()).toEqual({ presetId: "academic", modelIndex: 0, customInstruction: null, fresh: false });
    emit(PANEL_RESULT_EVENT, result({ requestId: "r2", presetId: "academic", text: "Academic text." }));
    expect(screen.getByTestId("panel-text")).toHaveTextContent("Academic text.");

    // Results are kept per preset + model; going back shows them without a request.
    press("1");
    expect(screen.getByTestId("panel-text")).toHaveTextContent("Polish text.");
    press("Tab");
    fireEvent.click(screen.getByRole("button", { name: "Gemini 3.8 Flash" }));
    expect(screen.getByTestId("panel-header")).toHaveTextContent("Gemini 3.8 Flash");
    expect(screen.getByTestId("panel-original")).toBeInTheDocument();
    // Toggling the ensemble mode on and off does not run anything either.
    press("e");
    await screen.findByTestId("ensemble-cost");
    press("e");
    await act(async () => undefined);
    expect(runCalls()).toHaveLength(2);
    expect(invokeCalls("panel_run_ensemble")).toHaveLength(0);

    fireEvent.click(startButton());
    await waitFor(() => expect(runCalls()).toHaveLength(3));
    expect(lastRun()).toEqual({ presetId: "polish", modelIndex: 2, customInstruction: null, fresh: false });
  });

  it("cycles models with Tab and Shift+Tab without running", async () => {
    await renderPanel();
    press("Tab");
    expect(screen.getByTestId("panel-header")).toHaveTextContent("Claude Opus 5.5 · medium");
    expect(screen.getByTestId("panel-header")).toHaveTextContent("Claude plan");
    expect(screen.getByRole("button", { name: "Claude Opus 5.5 · medium" })).toHaveAttribute("aria-pressed", "true");
    press("Tab", { shiftKey: true });
    press("Tab", { shiftKey: true });
    expect(screen.getByTestId("panel-header")).toHaveTextContent("Gemini 3.8 Flash");
    press("Tab");
    expect(screen.getByTestId("panel-header")).toHaveTextContent("GPT-6 Luna · low");
    await act(async () => undefined);
    expect(runCalls()).toHaveLength(0);
  });

  it("toggles a word diff with D", async () => {
    await startPanel();
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
    chord();
    await waitFor(() => expect(runCalls()).toHaveLength(1));
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
    await startPanel();
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
    expect(screen.getByText("Type an instruction above, then press Enter or Shift+Win.")).toBeInTheDocument();
    // A tap without an instruction asks for one.
    chord();
    expect(screen.getAllByText("Type an instruction first").length).toBeGreaterThan(0);

    // Keys typed in the input do not trigger panel shortcuts.
    fireEvent.keyDown(input, { key: "r" });
    fireEvent.keyDown(input, { key: "2" });

    // Up/down cycles the remembered instructions.
    fireEvent.keyDown(input, { key: "ArrowUp" });
    expect(input).toHaveValue("Translate to English");
    fireEvent.keyDown(input, { key: "ArrowUp" });
    expect(input).toHaveValue("Make it shorter");
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(input).toHaveValue("Translate to English");
    await act(async () => undefined);
    expect(runCalls()).toHaveLength(0);

    // Enter in the instruction starts it.
    fireEvent.change(input, { target: { value: "Make it friendlier" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(runCalls()).toHaveLength(1));
    expect(lastRun()).toEqual({ presetId: "custom", modelIndex: 0, customInstruction: "Make it friendlier", fresh: true });

    emit(PANEL_RESULT_EVENT, result({ requestId: "r1", presetId: "custom", text: "Friendly text." }));
    expect(screen.getByTestId("panel-text")).toHaveTextContent("Friendly text.");

    // Switching the model does not run; the next tap uses the instruction.
    press("Tab");
    await act(async () => undefined);
    expect(runCalls()).toHaveLength(1);
    chord();
    await waitFor(() => expect(runCalls()).toHaveLength(2));
    expect(lastRun()).toEqual({ presetId: "custom", modelIndex: 1, customInstruction: "Make it friendlier", fresh: true });
    press("Tab", { shiftKey: true });
    expect(screen.getByTestId("panel-text")).toHaveTextContent("Friendly text.");
    expect(runCalls()).toHaveLength(2);

    // Esc in the input still closes.
    fireEvent.keyDown(input, { key: "Escape" });
    expect(invoke).toHaveBeenCalledWith("panel_close");
  });

  it("prefills the newest remembered instruction when Custom was used last", async () => {
    initPayload = panelInit({ activePresetId: "custom" });
    await renderPanel();
    expect(screen.getByRole("textbox", { name: "Custom instruction" })).toHaveValue("Translate to English");
    await act(async () => undefined);
    expect(runCalls()).toHaveLength(0);
    chord();
    await waitFor(() => expect(runCalls()).toHaveLength(1));
    expect(lastRun()).toEqual({ presetId: "custom", modelIndex: 0, customInstruction: "Translate to English", fresh: true });
  });

  it("shows guard warnings as friendly notes", async () => {
    await startPanel();
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
    await startPanel();
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
    await renderPanel();
    chord();
    expect(await screen.findByText("No API key")).toBeInTheDocument();
  });
});

function invokeCalls(command: string) {
  return vi.mocked(invoke).mock.calls.filter(([name]) => name === command).map(([, args]) => args);
}

function card(number: number) {
  return screen.getByTestId(`ensemble-card-${number}`);
}

async function openCards() {
  await renderPanel();
  press("e");
  await screen.findByTestId("ensemble-cost");
  chord();
  await waitFor(() => expect(invokeCalls("panel_run_ensemble")).toHaveLength(1));
  // Let the run id resolve before member events arrive.
  await act(async () => undefined);
}

describe("AssistPanelView ensemble", () => {
  it("E shows members, providers, cost and limit without running; number keys pick profiles", async () => {
    await renderPanel();
    press("e");
    expect(invokeCalls("panel_ensemble_preview")).toEqual([{ profileId: "trio", presetId: "polish", customInstruction: null }]);
    const cost = await screen.findByTestId("ensemble-cost");
    expect(cost).toHaveTextContent("≈ $0.0123");
    expect(cost).toHaveTextContent("limit $0.05 per run");
    const members = screen.getByRole("list", { name: "Members" });
    expect(within(members).getAllByRole("listitem").map((item) => item.textContent)).toEqual([
      "GPT-6 Luna · lowAPI",
      "Claude Opus 5.5 · mediumClaude plan",
      "Gemini 3.8 FlashAPI",
    ]);
    expect(screen.getByText(/Sent to/).parentElement).toHaveTextContent("OpenAI, Claude plan (Claude Code), Google Gemini");
    expect(screen.getByRole("button", { name: "EEnsemble" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "1Trio" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "1Trio" })).toHaveAttribute("aria-keyshortcuts", "1");
    // Presets stay clickable here, but number keys pick profiles.
    expect(screen.getByRole("button", { name: "1Polish" })).not.toHaveAttribute("aria-keyshortcuts");
    expect(startButton()).toHaveAccessibleName("Start 3 models");
    expect(startButton()).toBeEnabled();
    press("2");
    await waitFor(() => expect(screen.getByTestId("ensemble-cost")).toHaveTextContent("≈ $0.002"));
    expect(invokeCalls("panel_ensemble_preview")).toHaveLength(2);
    expect(invokeCalls("panel_ensemble_preview")[1]).toMatchObject({ profileId: "duo" });
    expect(screen.getByText(/no known price/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "2Duo" })).toHaveAttribute("aria-pressed", "true");
    // Enter does not start an ensemble; only the tap or Start does.
    press("Enter");
    await act(async () => undefined);
    expect(runCalls()).toHaveLength(0);
    expect(invokeCalls("panel_run_ensemble")).toHaveLength(0);
  });

  it("re-estimates after a preset change in ensemble mode without running", async () => {
    await renderPanel();
    press("e");
    await screen.findByTestId("ensemble-cost");
    fireEvent.click(screen.getByRole("button", { name: "3Academic" }));
    await waitFor(() => expect(invokeCalls("panel_ensemble_preview")).toHaveLength(2));
    expect(invokeCalls("panel_ensemble_preview")[1]).toEqual({ profileId: "trio", presetId: "academic", customInstruction: null });
    await screen.findByTestId("ensemble-cost");
    chord();
    await waitFor(() => expect(invokeCalls("panel_run_ensemble")).toHaveLength(1));
    expect(invokeCalls("panel_run_ensemble")[0]).toEqual({ profileId: "trio", presetId: "academic", customInstruction: null });
    expect(runCalls()).toHaveLength(0);
  });

  it("cannot start an ensemble above the cost limit and says why", async () => {
    previews.trio = ensemblePreview({ estimatedUsd: 0.08, blocked: true });
    await renderPanel();
    press("e");
    expect(await screen.findByRole("alert")).toHaveTextContent("Over the cost limit");
    expect(screen.getByRole("alert")).toHaveTextContent("estimated at $0.08, above the per-run limit of $0.05");
    expect(startButton()).toBeDisabled();
    fireEvent.click(startButton());
    chord();
    press("Enter");
    await act(async () => undefined);
    expect(invokeCalls("panel_run_ensemble")).toHaveLength(0);
    expect(runCalls()).toHaveLength(0);
    expect(screen.getAllByText("Over the cost limit. This ensemble cannot start.").length).toBeGreaterThan(0);
  });

  it("a tap in ensemble mode runs the profile and shows one card per member, routed by member index", async () => {
    await openCards();
    expect(invokeCalls("panel_run_ensemble")).toEqual([{ profileId: "trio", presetId: "polish", customInstruction: null }]);
    expect(runCalls()).toHaveLength(0);
    expect(screen.getAllByTestId(/^ensemble-card-/)).toHaveLength(3);
    expect(card(1)).toHaveTextContent("GPT-6 Luna · low");
    expect(card(2)).toHaveTextContent("Claude Opus 5.5 · medium");
    expect(card(2)).toHaveTextContent("Claude plan");
    expect(within(card(3)).getByText("Waiting for the answer…")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Ensemble · 0 / 3");
    expect(screen.getByTestId("panel-drag-region")).toHaveAttribute("data-tauri-drag-region", "deep");

    emit(PANEL_RESULT_EVENT, member(1, { phase: "streaming", text: "Second card" }));
    expect(within(card(2)).getByTestId("ensemble-text-2")).toHaveTextContent("Second card");
    expect(within(card(1)).queryByText("Second card")).not.toBeInTheDocument();
    emit(PANEL_RESULT_EVENT, member(0, { phase: "done", text: "First card.", durationMs: 1400, costUsd: 0.0004 }));
    expect(card(1)).toHaveTextContent("API · ≈ $0.0004");
    expect(card(1)).toHaveTextContent("1.4 s");
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Ensemble · 1 / 3");

    // Other runs and sessions are ignored; members never reach the single view.
    emit(PANEL_RESULT_EVENT, member(2, { ensembleRunId: "old", phase: "done", text: "Old run" }));
    emit(PANEL_RESULT_EVENT, member(2, { sessionId: "s0", phase: "done", text: "Old session" }));
    expect(screen.queryByText("Old run")).not.toBeInTheDocument();
    expect(screen.queryByText("Old session")).not.toBeInTheDocument();
    press("Escape");
    expect(screen.queryByText("First card.")).not.toBeInTheDocument();
    expect(screen.getByTestId("panel-original")).toBeInTheDocument();
  });

  it("ignores a tap while the ensemble runs and runs it again once it finished", async () => {
    await openCards();
    expect(startButton()).toHaveAccessibleName("Running…");
    chord();
    await act(async () => undefined);
    expect(invokeCalls("panel_run_ensemble")).toHaveLength(1);
    expect(screen.getAllByText(/Already running/).length).toBeGreaterThan(0);
    emit(PANEL_RESULT_EVENT, member(0, { phase: "done", text: "One." }));
    emit(PANEL_RESULT_EVENT, member(1, { phase: "failed", error: "Busy" }));
    emit(PANEL_RESULT_EVENT, member(2, { phase: "done", text: "Three." }));
    expect(startButton()).toHaveAccessibleName("Run again");
    // Enter applies the selected card unchanged.
    press("Enter");
    expect(invokeCalls("panel_apply")).toEqual([{ requestId: "m0" }]);
  });

  it("runs the finished ensemble again on the next tap", async () => {
    await openCards();
    emit(PANEL_RESULT_EVENT, member(0, { phase: "done", text: "One." }));
    emit(PANEL_RESULT_EVENT, member(1, { phase: "done", text: "Two." }));
    emit(PANEL_RESULT_EVENT, member(2, { phase: "cancelled" }));
    runEnsemble = async () => "e2";
    chord();
    await waitFor(() => expect(invokeCalls("panel_run_ensemble")).toHaveLength(2));
    expect(invokeCalls("panel_run_ensemble")[1]).toEqual({ profileId: "trio", presetId: "polish", customInstruction: null });
  });

  it("keeps member events that arrive before the run id is known", async () => {
    let resolveRun!: (id: string) => void;
    runEnsemble = () => new Promise((resolve) => { resolveRun = resolve; });
    await renderPanel();
    press("e");
    await screen.findByTestId("ensemble-cost");
    chord();
    await waitFor(() => expect(invokeCalls("panel_run_ensemble")).toHaveLength(1));
    emit(PANEL_RESULT_EVENT, member(0, { phase: "done", text: "Early answer." }));
    emit(PANEL_RESULT_EVENT, member(1, { ensembleRunId: "other", phase: "done", text: "Other run." }));
    await act(async () => resolveRun("e1"));
    expect(within(card(1)).getByTestId("ensemble-text-1")).toHaveTextContent("Early answer.");
    expect(screen.queryByText("Other run.")).not.toBeInTheDocument();
  });

  it("selects cards with number keys, applies and copies the selected card", async () => {
    await openCards();
    emit(PANEL_RESULT_EVENT, member(0, { phase: "done", text: "First." }));
    emit(PANEL_RESULT_EVENT, member(1, { phase: "streaming", text: "Second" }));
    emit(PANEL_RESULT_EVENT, member(2, { phase: "done", text: "Third." }));
    expect(within(card(1)).getByRole("button", { pressed: true })).toBeInTheDocument();

    // Number keys select cards, not presets.
    press("3");
    expect(within(card(3)).getByRole("button", { pressed: true })).toBeInTheDocument();
    expect(within(card(1)).getByRole("button", { pressed: false })).toBeInTheDocument();
    expect(runCalls()).toHaveLength(0);
    press("9");
    expect(within(card(3)).getByRole("button", { pressed: true })).toBeInTheDocument();

    press("c", { ctrlKey: true });
    expect(invoke).toHaveBeenCalledWith("panel_copy", { requestId: "m2" });

    // A card that is still writing cannot be applied.
    press("2");
    press("Enter");
    expect(invokeCalls("panel_apply")).toHaveLength(0);

    fireEvent.click(within(card(3)).getByRole("button", { name: /^3\. Gemini 3\.8 Flash/ }));
    press("Enter");
    expect(invokeCalls("panel_apply")).toEqual([{ requestId: "m2" }]);
    press("Enter");
    expect(invokeCalls("panel_apply")).toHaveLength(1);
  });

  it("shows a failed member's own error while the others continue", async () => {
    await openCards();
    emit(PANEL_RESULT_EVENT, member(1, { phase: "failed", error: "Claude Code is not signed in" }));
    emit(PANEL_RESULT_EVENT, member(0, { phase: "streaming", text: "Still going" }));
    expect(within(card(2)).getByText("Claude Code is not signed in")).toBeInTheDocument();
    expect(within(card(2)).getAllByText("Failed").length).toBeGreaterThan(0);
    expect(within(card(1)).getByTestId("ensemble-text-1")).toHaveTextContent("Still going");
    expect(within(card(1)).queryByText(/not signed in/)).not.toBeInTheDocument();
    emit(PANEL_RESULT_EVENT, member(0, { phase: "done", text: "Finished." }));
    press("1");
    press("Enter");
    expect(invokeCalls("panel_apply")).toEqual([{ requestId: "m0" }]);
  });

  it("toggles a diff for the selected card and re-runs fresh with R", async () => {
    await openCards();
    emit(PANEL_RESULT_EVENT, member(0, { phase: "done", text: "The results show that the method is robust." }));
    emit(PANEL_RESULT_EVENT, member(2, { phase: "done", text: "Other text." }));
    press("d");
    const removed = [...within(card(1)).getByTestId("ensemble-diff-1").querySelectorAll("del")].map((node) => node.textContent?.trim());
    expect(removed).toContain("shows");
    expect(within(card(3)).queryByTestId("ensemble-diff-3")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "D diff" })).toHaveAttribute("aria-pressed", "true");
    press("d");
    expect(within(card(1)).getByTestId("ensemble-text-1")).toBeInTheDocument();

    runEnsemble = async () => "e2";
    press("r");
    await waitFor(() => expect(invokeCalls("panel_run_ensemble")).toHaveLength(2));
    await act(async () => undefined);
    expect(within(card(1)).queryByText(/The results show/)).not.toBeInTheDocument();
    emit(PANEL_RESULT_EVENT, member(0, { phase: "done", text: "Stale e1." }));
    expect(screen.queryByText("Stale e1.")).not.toBeInTheDocument();
    emit(PANEL_RESULT_EVENT, member(0, { ensembleRunId: "e2", phase: "done", text: "Fresh e2." }));
    expect(within(card(1)).getByTestId("ensemble-text-1")).toHaveTextContent("Fresh e2.");
  });

  it("returns to the single view with Esc and closes with a second Esc", async () => {
    await openCards();
    press("Escape");
    expect(screen.queryByTestId("ensemble-card-1")).not.toBeInTheDocument();
    expect(screen.getByTestId("panel-header")).toHaveTextContent("GPT-6 Luna · low");
    expect(invoke).not.toHaveBeenCalledWith("panel_close");
    // Number keys select presets again (without running).
    press("2");
    expect(screen.getByRole("button", { name: "2Clearer" })).toHaveAttribute("aria-pressed", "true");
    expect(runCalls()).toHaveLength(0);
    press("Escape");
    expect(invoke).toHaveBeenCalledWith("panel_close");
  });

  it("leaves the setup with Esc and shows why a run could not start", async () => {
    await renderPanel();
    press("e");
    await screen.findByTestId("ensemble-cost");
    press("Escape");
    expect(screen.queryByTestId("ensemble-cost")).not.toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("panel_close");

    runEnsemble = async () => { throw "Estimated $0.0600 exceeds the per-run limit of $0.05"; };
    press("e");
    await screen.findByTestId("ensemble-cost");
    chord();
    expect(await screen.findByText("Could not start the ensemble")).toBeInTheDocument();
    expect(screen.getAllByText(/exceeds the per-run limit/).length).toBeGreaterThan(0);
  });

  it("asks for a custom instruction and checks the cost before starting", async () => {
    await renderPanel();
    press("5");
    const input = screen.getByRole("textbox", { name: "Custom instruction" });
    await waitFor(() => expect(input).toHaveFocus());
    act(() => input.blur());
    press("e");
    expect(invokeCalls("panel_ensemble_preview")).toHaveLength(0);
    expect(screen.getByText("Type an instruction above, then press Enter.")).toBeInTheDocument();

    // The first Enter estimates the cost for the typed instruction; nothing runs yet.
    fireEvent.change(screen.getByRole("textbox", { name: "Custom instruction" }), { target: { value: "Make it friendlier" } });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Custom instruction" }), { key: "Enter" });
    expect(invokeCalls("panel_ensemble_preview")).toEqual([{ profileId: "trio", presetId: "custom", customInstruction: "Make it friendlier" }]);
    await screen.findByTestId("ensemble-cost");
    expect(invokeCalls("panel_run_ensemble")).toHaveLength(0);
    chord();
    await waitFor(() => expect(invokeCalls("panel_run_ensemble")).toHaveLength(1));
    expect(invokeCalls("panel_run_ensemble")[0]).toEqual({ profileId: "trio", presetId: "custom", customInstruction: "Make it friendlier" });
  });

  it("explains when there is no ensemble profile", async () => {
    initPayload = panelInit({ ensembleProfiles: [] });
    await renderPanel();
    expect(screen.getByRole("button", { name: "EEnsemble" })).toBeDisabled();
    press("e");
    expect(screen.getAllByText(/No ensemble models yet/).length).toBeGreaterThan(0);
    expect(invokeCalls("panel_ensemble_preview")).toHaveLength(0);
  });
});

describe("assistPanel helpers", () => {
  it("formats labels", () => {
    expect(billingLabel("chatgpt_plan")).toBe("ChatGPT plan");
    expect(billingLabel("local")).toBe("Local");
    expect(billingLabel("other")).toBe("other");
    expect(shortModelLabel("GPT-6 Luna (ChatGPT plan)", "ChatGPT plan")).toBe("GPT-6 Luna");
    expect(shortModelLabel("Claude Haiku (Claude plan)", "Claude plan")).toBe("Claude Haiku");
    expect(shortModelLabel("GPT-5.6 Luna", "API")).toBe("GPT-5.6 Luna");
    expect(shortModelLabel("Local model", "")).toBe("Local model");
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
    expect(normalizePanelResult(result())).not.toHaveProperty("memberIndex");
    expect(normalizePanelResult({ ...result(), ensembleRunId: "e1", memberIndex: 2 })).toMatchObject({ ensembleRunId: "e1", memberIndex: 2 });
    expect(normalizePanelResult({ ...result(), ensembleRunId: "e1", memberIndex: -1 })).not.toHaveProperty("ensembleRunId");
    const withProfiles = normalizePanelInit({ ...panelInit(), ensembleProfiles: [{ id: "x", name: "X", members: [] }, { id: "y", members: [LUNA] }] });
    expect(withProfiles?.ensembleProfiles).toEqual([{ id: "y", name: "y", members: [LUNA] }]);
    expect(normalizePanelInit({ ...panelInit(), ensembleProfiles: undefined, ensembleMaxUsd: undefined })).toMatchObject({ ensembleProfiles: [], ensembleMaxUsd: 0 });
  });

  it("validates ensemble previews and fails closed", () => {
    expect(normalizeEnsemblePreview(null)).toBeNull();
    expect(normalizeEnsemblePreview({ ...ensemblePreview(), profileId: "" })).toBeNull();
    expect(normalizeEnsemblePreview({ ...ensemblePreview(), blocked: undefined })?.blocked).toBe(true);
    expect(normalizeEnsemblePreview({ ...ensemblePreview(), estimatedUsd: Number.NaN })?.estimatedUsd).toBe(0);
    expect(formatUsd(0)).toBe("$0");
    expect(formatUsd(0.00004)).toBe("<$0.0001");
    expect(formatUsd(0.0123)).toBe("$0.0123");
    expect(formatUsd(0.05)).toBe("$0.05");
    expect(formatUsd(0.1)).toBe("$0.10");
    expect(formatUsd(1.234)).toBe("$1.23");
  });

  it("validates chord events and running phases", () => {
    expect(normalizePanelChord(null)).toBeNull();
    expect(normalizePanelChord({ kind: "tap" })).toBeNull();
    expect(normalizePanelChord({ sessionId: "s1", kind: "hold" })).toEqual({ sessionId: "s1", kind: "hold" });
    expect(normalizePanelChord({ sessionId: "s1", kind: "x" })).toEqual({ sessionId: "s1", kind: "tap" });
    expect(isRunningPhase("thinking")).toBe(true);
    expect(isRunningPhase("streaming")).toBe(true);
    expect(isRunningPhase("done")).toBe(false);
    expect(isRunningPhase(null)).toBe(false);
  });

  it("only lets a ready, unblocked preview for the current choice start an ensemble", () => {
    const choice = { profileId: "trio", presetId: "polish", customInstruction: null };
    const ready: PreviewState = { ...choice, status: "ready", preview: ensemblePreview() };
    expect(ensembleGate(ready, choice)).toMatchObject({ ok: true });
    expect(ensembleGate(null, choice)).toMatchObject({ ok: false, stale: true });
    expect(ensembleGate(ready, { ...choice, presetId: "academic" })).toMatchObject({ ok: false, stale: true });
    expect(ensembleGate(ready, { ...choice, profileId: "duo" })).toMatchObject({ ok: false, stale: true });
    expect(ensembleGate(ready, { ...choice, customInstruction: "x" })).toMatchObject({ ok: false, stale: true });
    expect(ensembleGate({ ...choice, status: "loading" }, choice)).toMatchObject({ ok: false, stale: false });
    expect(ensembleGate({ ...choice, status: "error", error: "No key" }, choice)).toEqual({ ok: false, reason: "No key", stale: false });
    expect(ensembleGate({ ...choice, status: "ready", preview: ensemblePreview({ blocked: true }) }, choice))
      .toMatchObject({ ok: false, stale: false, reason: expect.stringContaining("Over the cost limit") });
    expect(ensembleGate({ ...choice, status: "ready", preview: ensemblePreview({ members: [] }) }, choice)).toMatchObject({ ok: false, stale: false });
  });

  it("counts an ensemble as running until every card is final", () => {
    const run: EnsembleRun = {
      profileId: "trio",
      profileName: "Trio",
      presetId: "polish",
      customInstruction: null,
      members: [LUNA, OPUS],
      token: 1,
      runId: "e1",
      error: null,
    };
    expect(isEnsembleRunning(null, {})).toBe(false);
    expect(isEnsembleRunning(run, {})).toBe(true);
    expect(isEnsembleRunning(run, { 0: member(0, { phase: "done" }) })).toBe(true);
    expect(isEnsembleRunning(run, { 0: member(0, { phase: "done" }), 1: member(1, { phase: "failed" }) })).toBe(false);
    expect(isEnsembleRunning({ ...run, error: "Over the limit" }, {})).toBe(false);
  });
});
