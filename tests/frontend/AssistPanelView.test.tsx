import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
  formatUsd,
  normalizeEnsemblePreview,
  type EnsemblePreview,
  type PanelInit,
  type PanelModel,
  type PanelResult,
} from "../../src/lib/assistPanel";
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
  press("Enter");
  await waitFor(() => expect(invokeCalls("panel_run_ensemble")).toHaveLength(1));
  // Let the run id resolve before member events arrive.
  await act(async () => undefined);
}

describe("AssistPanelView ensemble", () => {
  it("opens a preview with members, providers, cost and limit; number keys pick profiles", async () => {
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
    expect(screen.getByRole("button", { name: "1Trio" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Enter run 3 models" })).toBeEnabled();
    // Number keys pick a profile here, not a preset.
    press("2");
    await waitFor(() => expect(screen.getByTestId("ensemble-cost")).toHaveTextContent("≈ $0.002"));
    expect(invokeCalls("panel_ensemble_preview")).toHaveLength(2);
    expect(invokeCalls("panel_ensemble_preview")[1]).toMatchObject({ profileId: "duo" });
    expect(screen.getByText(/no known price/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "2Duo" })).toHaveAttribute("aria-pressed", "true");
    expect(runCalls()).toHaveLength(1);
  });

  it("blocks a run above the cost limit and offers no run button", async () => {
    previews.trio = ensemblePreview({ estimatedUsd: 0.08, blocked: true });
    await renderPanel();
    press("e");
    expect(await screen.findByRole("alert")).toHaveTextContent("Over the cost limit");
    expect(screen.getByRole("alert")).toHaveTextContent("estimated at $0.08, above the per-run limit of $0.05");
    expect(screen.queryByRole("button", { name: /run/ })).not.toBeInTheDocument();
    press("Enter");
    press("e");
    expect(invokeCalls("panel_run_ensemble")).toHaveLength(0);
  });

  it("runs the profile and shows one card per member, routed by member index", async () => {
    await openCards();
    expect(invokeCalls("panel_run_ensemble")).toEqual([{ profileId: "trio", presetId: "polish", customInstruction: null }]);
    expect(screen.getAllByTestId(/^ensemble-card-/)).toHaveLength(3);
    expect(card(1)).toHaveTextContent("GPT-6 Luna · low");
    expect(card(2)).toHaveTextContent("Claude Opus 5.5 · medium");
    expect(card(2)).toHaveTextContent("Claude plan");
    expect(within(card(3)).getByText("Waiting for the answer…")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Ensemble · 0 / 3");

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
    expect(screen.getByText("Thinking…")).toBeInTheDocument();
  });

  it("keeps member events that arrive before the run id is known", async () => {
    let resolveRun!: (id: string) => void;
    runEnsemble = () => new Promise((resolve) => { resolveRun = resolve; });
    await renderPanel();
    press("e");
    await screen.findByTestId("ensemble-cost");
    press("Enter");
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
    expect(runCalls()).toHaveLength(1);
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
    // Number keys select presets again.
    press("2");
    await waitFor(() => expect(lastRun()).toMatchObject({ presetId: "clearer" }));
    press("Escape");
    expect(invoke).toHaveBeenCalledWith("panel_close");
  });

  it("leaves the preview with Esc and shows why a run could not start", async () => {
    await renderPanel();
    press("e");
    await screen.findByTestId("ensemble-cost");
    press("Escape");
    expect(screen.queryByTestId("ensemble-cost")).not.toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("panel_close");

    runEnsemble = async () => { throw "Estimated $0.0600 exceeds the per-run limit of $0.05"; };
    press("e");
    await screen.findByTestId("ensemble-cost");
    press("e");
    expect(await screen.findByText("Could not start the ensemble")).toBeInTheDocument();
    expect(screen.getAllByText(/exceeds the per-run limit/).length).toBeGreaterThan(0);
  });

  it("uses the last custom instruction and asks for one first", async () => {
    await renderPanel();
    press("5");
    const input = screen.getByRole("textbox", { name: "Custom instruction" });
    await waitFor(() => expect(input).toHaveFocus());
    act(() => input.blur());
    press("e");
    expect(invokeCalls("panel_ensemble_preview")).toHaveLength(0);
    expect(screen.getAllByText("Run a custom instruction first, then press E").length).toBeGreaterThan(0);

    fireEvent.change(input, { target: { value: "Make it friendlier" } });
    fireEvent.keyDown(input, { key: "Enter" });
    press("e");
    expect(invokeCalls("panel_ensemble_preview")).toEqual([{ profileId: "trio", presetId: "custom", customInstruction: "Make it friendlier" }]);
  });

  it("explains when there is no ensemble profile", async () => {
    initPayload = panelInit({ ensembleProfiles: [] });
    await renderPanel();
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
});
