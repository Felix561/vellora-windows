import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsView } from "../../src/views/SettingsView";
import { TextAssistSettingsView } from "../../src/views/TextAssistSettingsView";
import { HistoryView } from "../../src/views/HistoryView";
import * as api from "../../src/lib/api";
import type { AppSettings } from "../../src/lib/types";
import { settings, stats, textAssistOverview, textAssistRun, textAssistSettings, textProviderKeys, transcript } from "./fixtures";

vi.mock("../../src/lib/api", () => ({
  getStartAtLogin: vi.fn(),
  setStartAtLogin: vi.fn(),
  saveOpenAiApiKey: vi.fn(),
  deleteOpenAiApiKey: vi.fn(),
  clearTranscripts: vi.fn(),
  copyTranscript: vi.fn(),
  pasteTranscript: vi.fn(),
  deleteTranscript: vi.fn(),
  getDashboardStats: vi.fn(),
  getTextAssistOverview: vi.fn(),
  listTextAssistRuns: vi.fn(),
  deleteTextAssistRun: vi.fn(),
  clearTextAssistRuns: vi.fn(),
  copyTextAssistText: vi.fn(),
  onTextAssistRunRecorded: vi.fn(),
  getTextProviderKeys: vi.fn(),
}));

let runRecorded: ((runId: string) => void) | null;

beforeEach(() => {
  runRecorded = null;
  vi.mocked(api.getStartAtLogin).mockResolvedValue(false);
  vi.mocked(api.getDashboardStats).mockResolvedValue(stats());
  vi.mocked(api.getTextAssistOverview).mockResolvedValue(textAssistOverview());
  vi.mocked(api.getTextProviderKeys).mockResolvedValue(textProviderKeys({ gemini: true }));
  vi.mocked(api.listTextAssistRuns).mockResolvedValue([]);
  vi.mocked(api.deleteTextAssistRun).mockResolvedValue(true);
  vi.mocked(api.clearTextAssistRuns).mockResolvedValue(2);
  vi.mocked(api.copyTextAssistText).mockResolvedValue();
  vi.mocked(api.onTextAssistRunRecorded).mockImplementation(async (handler) => {
    runRecorded = handler;
    return () => { runRecorded = null; };
  });
});

async function openTextAssistSettings(props: Partial<{ saved: AppSettings; hasKey: boolean; onSettingsChange: (patch: Partial<AppSettings>) => Promise<void> }> = {}) {
  const user = userEvent.setup();
  const onSettingsChange = props.onSettingsChange ?? vi.fn().mockResolvedValue(undefined);
  const view = render(<SettingsView settings={props.saved ?? settings()} hasKey={props.hasKey ?? true} onSettingsChange={onSettingsChange} onKeyStatusChange={vi.fn()} />);
  await user.click(screen.getByRole("tab", { name: "Text Assist" }));
  await screen.findByText(/GPT-6 Luna · None effort/);
  return { user, onSettingsChange, ...view };
}

function quickSummary() {
  return screen.getByText((_, element) => element?.classList.contains("ta-model-summary-text") ?? false);
}

describe("Text Assist settings", () => {
  it("explains Quick Polish and the Rewrite panel flow", async () => {
    await openTextAssistSettings();
    expect(screen.getByRole("tabpanel", { name: "Text Assist" })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Enable Text Assist" })).toBeChecked();
    expect(screen.queryByText(/coming soon/)).not.toBeInTheDocument();
    expect(screen.getByText(/select text in any app and tap/)).toHaveTextContent(
      "Quick Polish: select text in any app and tap Shift+Win to fix spelling, grammar and punctuation in place.",
    );
    const steps = within(screen.getByRole("list", { name: "Rewrite panel steps" })).getAllByRole("listitem").map((item) => item.textContent);
    expect(steps).toEqual([
      "Select text and hold Shift+Win to open the panel. Nothing is sent yet.",
      "Pick a style and a model, or an ensemble.",
      "Tap Shift+Win to start.",
      "Press Enter to apply the result, or Esc to close.",
    ]);
    expect(screen.getByText(/Only the selected text is sent, to the provider you chose\./)).toBeInTheDocument();
  });

  it("summarizes the models instead of duplicating the picker", async () => {
    await openTextAssistSettings({ saved: settings({ textAssist: textAssistSettings({ ensembleProfiles: [{ id: "a", name: "A", members: [{ provider: "openai", model: "gpt-6-luna" }] }] }) }) });
    expect(quickSummary()).toHaveTextContent("Quick Polish: GPT-6 Luna · None effort · $0.20 in / $0.80 out per 1M");
    expect(quickSummary()).toHaveTextContent("Rewrite panel: 1 model · Ensembles: 1 profile");
    const panel = screen.getByRole("tabpanel", { name: "Text Assist" });
    expect(within(panel).queryByRole("combobox")).not.toBeInTheDocument();
    expect(within(panel).queryByRole("radiogroup")).not.toBeInTheDocument();
    // SettingsView gives no navigation callback here, so the view names the place instead of offering a button.
    expect(screen.queryByRole("button", { name: /Choose models/ })).not.toBeInTheDocument();
    expect(screen.getByText(/Choose models and efforts for Quick Polish, the Rewrite panel and ensembles in AI Models › Text models\./)).toBeInTheDocument();
    expect(api.getTextProviderKeys).toHaveBeenCalledWith([]);
  });

  it("links to AI Models when a navigation callback is given", async () => {
    const user = userEvent.setup();
    const onOpenTextModels = vi.fn();
    render(<TextAssistSettingsView settings={settings({ textAssist: textAssistSettings({ favoriteModels: [], quickModel: { provider: "anthropic", model: "claude-haiku-5-5", effort: "none" } }) })}
      hasKey onSettingsChange={vi.fn()} onOpenTextModels={onOpenTextModels} />);
    expect(await screen.findByText(/Claude Haiku 5\.5 · None effort/)).toBeInTheDocument();
    expect(quickSummary()).toHaveTextContent("Rewrite panel: 0 models · Ensembles: Favorites (automatic)");
    const button = screen.getByRole("button", { name: "Choose models" });
    expect(button).toHaveAccessibleDescription(/Quick Polish: Claude Haiku 5\.5/);
    await user.click(button);
    expect(onOpenTextModels).toHaveBeenCalledTimes(1);

    // Anthropic has no key in the fixture: the note says where to fix it and links there too.
    expect(await screen.findByText(/Quick Polish cannot run yet\. No Anthropic API key is saved\. Add it under AI Models › Text models › Providers & keys\./)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Open AI Models" }));
    expect(onOpenTextModels).toHaveBeenCalledTimes(2);
  });

  it("notes a missing OpenAI key with the place to add it", async () => {
    await openTextAssistSettings({ hasKey: false });
    expect(screen.getByText(/Quick Polish cannot run yet\. No OpenAI API key is saved\. Add it under Settings › General › OpenAI key/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open AI Models" })).not.toBeInTheDocument();
  });

  it("validates limits before saving them", async () => {
    const { user, onSettingsChange } = await openTextAssistSettings();
    const maxChars = screen.getByRole("spinbutton", { name: "Quick Polish max characters" });
    await user.clear(maxChars);
    await user.type(maxChars, "60000");
    expect(maxChars).toHaveAttribute("aria-invalid", "true");
    expect(maxChars).toHaveAccessibleDescription("Enter a whole number from 100 to 50,000 characters.");
    expect(screen.getByRole("button", { name: "Save limits" })).toBeDisabled();

    const delay = screen.getByRole("spinbutton", { name: "Paste restore delay (ms)" });
    await user.clear(delay);
    await user.type(delay, "20");
    expect(screen.getByText("Enter a whole number from 50 to 5,000 milliseconds.")).toBeInTheDocument();

    await user.clear(maxChars);
    await user.type(maxChars, "12000");
    await user.clear(delay);
    await user.type(delay, "600");
    await user.click(screen.getByRole("button", { name: "Save limits" }));
    expect(onSettingsChange).toHaveBeenCalledWith({
      textAssist: textAssistSettings({ limits: { quickMaxChars: 12000, panelMaxChars: 30000, ensembleMaxUsdPerRun: 0.05, pasteRestoreDelayMs: 600 } }),
    });
  });

  it("saves the history and developer toggles", async () => {
    const { user, onSettingsChange } = await openTextAssistSettings();
    await user.click(screen.getByRole("checkbox", { name: "Save Text Assist history" }));
    expect(onSettingsChange).toHaveBeenLastCalledWith({ textAssist: textAssistSettings({ saveHistory: false }) });

    await waitFor(() => expect(screen.getByRole("checkbox", { name: "Save Text Assist history" })).toBeEnabled());
    await user.click(screen.getByText("Advanced"));
    await user.click(screen.getByRole("checkbox", { name: "Offline test transform (uppercase, no network)" }));
    expect(onSettingsChange).toHaveBeenLastCalledWith({ textAssist: textAssistSettings({ devOfflineTransform: true }) });
  });

  it("reports a failed save", async () => {
    await openTextAssistSettings({ onSettingsChange: vi.fn().mockRejectedValue(new Error("Disk is read-only")) }).then(async ({ user }) => {
      await user.click(screen.getByRole("checkbox", { name: "Enable Text Assist" }));
    });
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save Text Assist settings: Error: Disk is read-only");
    expect(screen.getByRole("checkbox", { name: "Enable Text Assist" })).toBeChecked();
  });
});

async function openTextAssistHistory(saveHistory = true) {
  const user = userEvent.setup();
  render(<HistoryView items={[transcript()]} onRefresh={vi.fn().mockResolvedValue(undefined)} textAssistSaveHistory={saveHistory} />);
  expect(screen.getByText("A recoverable dictation.")).toBeInTheDocument();
  await user.click(screen.getByRole("tab", { name: "Text Assist" }));
  return user;
}

describe("Text Assist history", () => {
  it("lists runs with outcome badges and shows the word diff for a selected run", async () => {
    vi.mocked(api.listTextAssistRuns).mockResolvedValue([
      textAssistRun(),
      textAssistRun({ id: "run-2", createdAt: "2026-11-17T09:00:00Z", outcome: "kept", reasonCode: "target_changed", targetApp: "Code.exe", effort: "low", estimatedCostUsd: null }),
      textAssistRun({ id: "run-3", createdAt: "2026-11-17T08:00:00Z", outcome: "failed", reasonCode: "rate_limited", resultText: null, targetApp: "texstudio.exe" }),
    ]);
    const user = await openTextAssistHistory();
    const list = await screen.findByRole("list", { name: "Text Assist runs" });
    const runs = within(list).getAllByRole("button");
    expect(runs).toHaveLength(3);
    expect(runs[0]).toHaveTextContent("notepad.exe");
    expect(runs[0]).toHaveTextContent("Outcome: Applied");
    expect(runs[0]).toHaveTextContent("gpt-6-luna · None");
    expect(runs[0]).toHaveTextContent("Quick Polish");
    expect(runs[0]).toHaveTextContent("1.2s");
    expect(runs[0]).toHaveTextContent("<$0.0001");
    expect(runs[1]).toHaveTextContent("Outcome: Kept");
    expect(runs[1]).toHaveTextContent("The target window or selection changed, so nothing was pasted.");
    expect(runs[2]).toHaveTextContent("Outcome: Failed");
    expect(runs[2]).toHaveTextContent("The provider's rate limit was reached.");
    expect(api.listTextAssistRuns).toHaveBeenCalledWith(100);

    await user.click(runs[0]);
    expect(runs[0]).toHaveAttribute("aria-pressed", "true");
    const details = screen.getByRole("article", { name: "Run details" });
    const diff = within(details).getByLabelText("Word changes from original to result");
    expect(within(diff).getAllByRole("deletion").map((node) => node.textContent?.trim())).toEqual(["[removed: Teh]", "[removed: jump.]"]);
    expect(within(diff).getAllByRole("insertion").map((node) => node.textContent?.trim())).toEqual(["[added: The]", "[added: jumps.]"]);
    expect(within(details).getByText("(2 changes)")).toBeInTheDocument();

    await user.click(within(details).getByRole("button", { name: "Copy result" }));
    expect(api.copyTextAssistText).toHaveBeenCalledWith("The quick brown fox jumps.");
    expect(await screen.findByRole("status")).toHaveTextContent("Result copied to the clipboard.");

    await user.click(runs[2]);
    await user.click(within(screen.getByRole("article", { name: "Run details" })).getByRole("button", { name: "Copy original" }));
    expect(api.copyTextAssistText).toHaveBeenLastCalledWith("Teh quick brown fox jump.");
  });

  it("deletes a run only after confirmation", async () => {
    vi.mocked(api.listTextAssistRuns).mockResolvedValueOnce([textAssistRun()]).mockResolvedValue([]);
    const user = await openTextAssistHistory();
    await user.click(await screen.findByRole("button", { name: /notepad\.exe/ }));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    const confirmation = screen.getByRole("group", { name: "Confirm Text Assist run deletion" });
    expect(within(confirmation).getByRole("button", { name: "Cancel" })).toHaveFocus();
    expect(api.deleteTextAssistRun).not.toHaveBeenCalled();

    await user.click(within(confirmation).getByRole("button", { name: "Delete run" }));
    expect(api.deleteTextAssistRun).toHaveBeenCalledWith("run-1");
    expect(await screen.findByRole("status")).toHaveTextContent("Text Assist run deleted.");
    expect(screen.queryByRole("button", { name: /notepad\.exe/ })).not.toBeInTheDocument();
  });

  it("keeps the run and reports a failed deletion", async () => {
    vi.mocked(api.listTextAssistRuns).mockResolvedValue([textAssistRun()]);
    vi.mocked(api.deleteTextAssistRun).mockRejectedValue(new Error("Database unavailable"));
    const user = await openTextAssistHistory();
    await user.click(await screen.findByRole("button", { name: /notepad\.exe/ }));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete run" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not delete: Error: Database unavailable");
    expect(screen.getByRole("button", { name: /notepad\.exe/ })).toBeInTheDocument();
  });

  it("clears all Text Assist runs after confirmation", async () => {
    vi.mocked(api.listTextAssistRuns).mockResolvedValueOnce([textAssistRun(), textAssistRun({ id: "run-2" })]).mockResolvedValue([]);
    const user = await openTextAssistHistory();
    await screen.findByRole("list", { name: "Text Assist runs" });
    await user.click(screen.getByRole("button", { name: "Clear Text Assist history" }));
    const confirmation = screen.getByRole("group", { name: "Confirm clearing Text Assist history" });
    await user.click(within(confirmation).getByRole("button", { name: "Cancel" }));
    expect(api.clearTextAssistRuns).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Clear Text Assist history" }));
    await user.click(screen.getByRole("button", { name: "Delete all Text Assist runs" }));
    expect(api.clearTextAssistRuns).toHaveBeenCalledOnce();
    expect(await screen.findByRole("status")).toHaveTextContent("Deleted 2 Text Assist runs.");
    expect(screen.queryByRole("list", { name: "Text Assist runs" })).not.toBeInTheDocument();
  });

  it("refreshes when the backend records a new run", async () => {
    await openTextAssistHistory();
    expect(await screen.findByText(/No Text Assist runs yet/)).toBeInTheDocument();
    vi.mocked(api.listTextAssistRuns).mockResolvedValue([textAssistRun()]);
    await act(async () => { runRecorded?.("run-1"); });
    expect(await screen.findByRole("button", { name: /notepad\.exe/ })).toBeInTheDocument();
  });

  it("offers the latest kept result when history saving is off", async () => {
    const kept = textAssistRun({ id: "kept", outcome: "kept", reasonCode: "focus_changed", targetApp: "Code.exe", resultText: "Fixed text." });
    vi.mocked(api.getTextAssistOverview).mockResolvedValue(textAssistOverview({ lastKept: kept }));
    const user = await openTextAssistHistory(false);
    const banner = await screen.findByRole("region", { name: "Latest kept result" });
    expect(banner).toHaveTextContent("Result ready from Code.exe");
    await user.click(within(banner).getByRole("button", { name: "Copy" }));
    expect(api.copyTextAssistText).toHaveBeenCalledWith("Fixed text.");
  });

  it("hides the kept banner when the kept run is already listed", async () => {
    const kept = textAssistRun({ id: "kept", outcome: "kept" });
    vi.mocked(api.listTextAssistRuns).mockResolvedValue([kept]);
    vi.mocked(api.getTextAssistOverview).mockResolvedValue(textAssistOverview({ lastKept: kept }));
    await openTextAssistHistory();
    await screen.findByRole("list", { name: "Text Assist runs" });
    expect(screen.queryByRole("region", { name: "Latest kept result" })).not.toBeInTheDocument();
  });
});
