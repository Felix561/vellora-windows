import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsView } from "../../src/views/SettingsView";
import { HistoryView } from "../../src/views/HistoryView";
import { DashboardView } from "../../src/views/DashboardView";
import * as api from "../../src/lib/api";
import type { AppSettings } from "../../src/lib/types";
import { health, settings, stats, textAssistOverview, textAssistRun, textAssistSettings, transcript } from "./fixtures";

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
}));

let runRecorded: ((runId: string) => void) | null;

beforeEach(() => {
  runRecorded = null;
  vi.mocked(api.getStartAtLogin).mockResolvedValue(false);
  vi.mocked(api.getDashboardStats).mockResolvedValue(stats());
  vi.mocked(api.getTextAssistOverview).mockResolvedValue(textAssistOverview());
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
  await waitFor(() => expect(screen.getByRole("combobox", { name: "Model" })).toBeEnabled());
  return { user, onSettingsChange, ...view };
}

describe("Text Assist settings", () => {
  it("renders the section with OpenAI models only, quick candidates first", async () => {
    await openTextAssistSettings();
    expect(screen.getByRole("tabpanel", { name: "Text Assist" })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Enable Text Assist" })).toBeChecked();
    expect(screen.getByText(/Select text in any app and tap/)).toHaveTextContent(
      "Select text in any app and tap Shift+Win to fix spelling, grammar and punctuation in place. Hold Shift+Win for the Rewrite panel (coming soon).",
    );
    const options = within(screen.getByRole("combobox", { name: "Model" })).getAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual([
      "GPT-6 Luna - $0.20 in / $0.80 out per 1M tokens",
      "GPT-5.6 Luna - $1.00 in / $4.00 out per 1M tokens",
    ]);
    expect(screen.getByRole("combobox", { name: "Model" })).toHaveValue("gpt-6-luna");
    expect(screen.getByText(/Only the selected text is sent, to the provider you chose\./)).toBeInTheDocument();
    expect(screen.queryByText(/needs an OpenAI API key/)).not.toBeInTheDocument();
  });

  it("limits effort options to the model and saves the whole textAssist block", async () => {
    const { user, onSettingsChange } = await openTextAssistSettings();
    const effort = screen.getByRole("combobox", { name: "Reasoning effort" });
    expect(within(effort).getAllByRole("option").map((option) => option.getAttribute("value"))).toEqual(["none", "low", "medium"]);

    await user.selectOptions(effort, "low");
    expect(onSettingsChange).toHaveBeenLastCalledWith({
      textAssist: textAssistSettings({ quickModel: { provider: "openai", model: "gpt-6-luna", effort: "low" } }),
    });

    // "none" is not offered by GPT-5.6 Luna, so switching falls back to its default effort.
    await waitFor(() => expect(screen.getByRole("combobox", { name: "Model" })).toBeEnabled());
    await user.selectOptions(screen.getByRole("combobox", { name: "Model" }), "gpt-5.6-luna");
    expect(onSettingsChange).toHaveBeenLastCalledWith({
      textAssist: textAssistSettings({ quickModel: { provider: "openai", model: "gpt-5.6-luna", effort: "low" } }),
    });
    expect(await screen.findByRole("status")).toHaveTextContent("Text Assist settings saved.");
  });

  it("offers only the saved model's efforts after it is saved", async () => {
    await openTextAssistSettings({ saved: settings({ textAssist: textAssistSettings({ quickModel: { provider: "openai", model: "gpt-5.6-luna", effort: "medium" } }) }) });
    const effort = screen.getByRole("combobox", { name: "Reasoning effort" });
    expect(within(effort).getAllByRole("option").map((option) => option.textContent)).toEqual(["Low", "Medium", "High"]);
    expect(effort).toHaveValue("medium");
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

  it("saves the history and developer toggles and notes a missing key", async () => {
    const { user, onSettingsChange } = await openTextAssistSettings({ hasKey: false });
    expect(screen.getByText(/Quick Polish needs an OpenAI API key/)).toBeInTheDocument();
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

describe("Text Assist dashboard card", () => {
  it("shows totals separately from dictation spend", async () => {
    vi.mocked(api.getTextAssistOverview).mockResolvedValue(textAssistOverview({
      totals: { runs: 42, appliedRuns: 37, estimatedCostUsd: 0.1234, estimatedCostTodayUsd: 0.004, planRuns: 3 },
    }));
    render(<DashboardView settings={settings()} hasKey snapshot={{ state: "idle" }} history={[]} health={health()} diagnostics={[]} />);
    const card = await screen.findByRole("region", { name: "Text Assist" });
    await waitFor(() => expect(within(card).getByText("42")).toBeInTheDocument());
    expect(within(card).getByText("Runs")).toBeInTheDocument();
    expect(within(card).getByText("37")).toBeInTheDocument();
    expect(within(card).getByText("<$0.01")).toBeInTheDocument();
    expect(within(card).getByText("$0.12")).toBeInTheDocument();
    expect(within(card).getByText("3")).toBeInTheDocument();
    expect(card).toHaveTextContent("Estimates are not a billing ledger.");
  });

  it("reports when totals cannot be loaded", async () => {
    vi.mocked(api.getTextAssistOverview).mockRejectedValue(new Error("Database unavailable"));
    render(<DashboardView settings={settings()} hasKey snapshot={{ state: "idle" }} history={[]} health={health()} diagnostics={[]} />);
    const card = await screen.findByRole("region", { name: "Text Assist" });
    expect(await within(card).findByRole("alert")).toHaveTextContent("Could not load Text Assist totals: Error: Database unavailable");
  });
});
