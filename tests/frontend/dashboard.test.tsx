import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DashboardView } from "../../src/views/DashboardView";
import * as api from "../../src/lib/api";
import { emptyUsageOverview, emptyUsagePeriod, formatMinutesSaved, reasonLabel } from "../../src/lib/usageStats";
import type { UsageOverview, UsagePeriod } from "../../src/lib/types";
import { health, settings, transcript } from "./fixtures";

vi.mock("../../src/lib/api", () => ({
  copyTranscript: vi.fn(),
  pasteTranscript: vi.fn(),
  deleteTranscript: vi.fn(),
  getUsageOverview: vi.fn(),
  onTextAssistRunRecorded: vi.fn(),
}));

let runRecorded: ((runId: string) => void) | null;

/** Numbers render in the runtime locale. */
const n = (value: number) => value.toLocaleString();

function period(overrides: { dictation?: Partial<UsagePeriod["dictation"]>; textAssist?: Partial<UsagePeriod["textAssist"]>; insights?: Partial<UsagePeriod["insights"]> } = {}): UsagePeriod {
  const base = emptyUsagePeriod();
  return {
    dictation: { ...base.dictation, ...overrides.dictation },
    textAssist: { ...base.textAssist, ...overrides.textAssist },
    insights: { ...base.insights, ...overrides.insights },
  };
}

function overview(): UsageOverview {
  const value = emptyUsageOverview("2026-11-18");
  value.periods.today = period({
    dictation: { transcripts: 2, words: 120, minutesSaved: 3, estimatedCostUsd: 0.004 },
    textAssist: { runs: 3, applied: 2, failed: 1, wordsProcessed: 40, wordsCorrected: 5, minutesSaved: 0.5, estimatedCostUsd: 0.002 },
    insights: {
      outcomes: { applied: 2, kept: 0, noChange: 0, cancelled: 0, failed: 1, other: 0 },
      returnedResults: 2,
      applyRate: 1,
      duration: { count: 2, averageMs: 900, medianMs: 850 },
      firstToken: { count: 2, averageMs: 320, medianMs: 300 },
      models: [{ provider: "openai", model: "gpt-6-luna", effort: "none", billing: "api", runs: 2, applied: 2, failed: 0, averageDurationMs: 900, estimatedCostUsd: 0.002 }],
      apps: [
        { targetApp: "notepad.exe", runs: 2, applied: 2, failed: 0 },
        { targetApp: null, runs: 1, applied: 0, failed: 1 },
      ],
      presets: [{ kind: "polish", presetId: "polish", runs: 2, applied: 2 }],
      failureReasons: [{ code: "no_selection", count: 1 }],
      keptReasons: [],
    },
  });
  value.periods.last7Days = period({
    dictation: { transcripts: 14, words: 4321, minutesSaved: 108, estimatedCostUsd: 0.09 },
    textAssist: { runs: 30, applied: 21, failed: 4, wordsProcessed: 900, wordsCorrected: 87, minutesSaved: 8.7, estimatedCostUsd: 0.03, planRuns: 5, ensembleRuns: 2 },
    insights: {
      outcomes: { applied: 21, kept: 3, noChange: 1, cancelled: 1, failed: 4, other: 0 },
      returnedResults: 25,
      applyRate: 0.84,
      duration: { count: 25, averageMs: 1400, medianMs: 1100 },
      firstToken: { count: 25, averageMs: 400, medianMs: 350 },
      models: [
        { provider: "openai", model: "gpt-6-luna", effort: "none", billing: "api", runs: 20, applied: 16, failed: 1, averageDurationMs: 1000, estimatedCostUsd: 0.03 },
        { provider: "codex_cli", model: "gpt-6", effort: null, billing: "chatgpt_plan", runs: 5, applied: 3, failed: 0, averageDurationMs: 4200, estimatedCostUsd: 0 },
      ],
      apps: [
        { targetApp: "code.exe", runs: 18, applied: 12, failed: 2 },
        { targetApp: "winword.exe", runs: 8, applied: 7, failed: 0 },
      ],
      presets: [
        { kind: "polish", presetId: "polish", runs: 22, applied: 17 },
        { kind: "rewrite", presetId: "formal", runs: 3, applied: 2 },
        { kind: "ensemble", presetId: "polish", runs: 5, applied: 2 },
      ],
      failureReasons: [
        { code: "clipboard_busy", count: 2 },
        { code: "no_selection", count: 1 },
        { code: "weird_new_code", count: 1 },
      ],
      keptReasons: [{ code: "guard_preamble", count: 3 }],
    },
  });
  value.periods.last30Days = value.periods.last7Days;
  value.periods.allTime = value.periods.last7Days;
  value.daily[29] = { ...value.daily[29], transcripts: 2, dictatedWords: 120, textAssistRuns: 3, textAssistApplied: 2, wordsCorrected: 5 };
  value.daily[27] = { ...value.daily[27], transcripts: 12, dictatedWords: 4201, textAssistRuns: 27, textAssistApplied: 19, wordsCorrected: 82 };
  value.dictationEstimatedFromWordsCount = 2;
  return value;
}

function renderDashboard(history = [transcript()]) {
  return render(<DashboardView settings={settings()} hasKey snapshot={{ state: "idle" }} history={history} health={health()} diagnostics={[]} />);
}

beforeEach(() => {
  runRecorded = null;
  vi.mocked(api.getUsageOverview).mockReset();
  vi.mocked(api.getUsageOverview).mockResolvedValue(overview());
  vi.mocked(api.onTextAssistRunRecorded).mockImplementation(async (handler) => {
    runRecorded = handler;
    return () => { runRecorded = null; };
  });
});

describe("dashboard usage overview", () => {
  it("shows dictation and Text Assist side by side with totals for the last 7 days", async () => {
    renderDashboard();
    const usage = await screen.findByRole("region", { name: "Your usage" });
    await within(usage).findByText(n(4321));
    const dictation = within(usage).getByRole("region", { name: "Dictation" });
    const assist = within(usage).getByRole("region", { name: "Text Assist" });
    const together = within(usage).getByRole("region", { name: "Together" });

    expect(within(dictation).getByText("Words dictated").nextSibling).toHaveTextContent(n(4321));
    expect(within(dictation).getByText("Transcriptions").nextSibling).toHaveTextContent("14");
    expect(within(dictation).getByText("Time saved").nextSibling).toHaveTextContent("1 h 48 min");
    expect(within(dictation).getByText("Estimated spend").nextSibling).toHaveTextContent("$0.09");
    expect(within(assist).getByText("Texts fixed (applied)").nextSibling).toHaveTextContent("21 of 30 runs");
    expect(within(assist).getByText("Words corrected").nextSibling).toHaveTextContent("87");
    expect(within(assist).getByText("Time saved").nextSibling).toHaveTextContent("9 min");
    expect(within(assist).getByText("Estimated spend").nextSibling).toHaveTextContent("$0.03");
    expect(assist).toHaveTextContent("900 words processed / 5 plan runs");
    expect(within(together).getByText("Time saved").nextSibling).toHaveTextContent("1 h 57 min");
    expect(within(together).getByText("Estimated spend").nextSibling).toHaveTextContent("$0.12");
    expect(within(together).getByText("Uses").nextSibling).toHaveTextContent("44");

    expect(usage).toHaveTextContent("1 minute per 40 dictated words, and 6 seconds per corrected word");
    expect(usage).toHaveTextContent("Not official billing data.");
    expect(usage).toHaveTextContent("These are estimates, not billing data.");
    expect(usage).toHaveTextContent("2 older notes estimated from word count.");
    // The separate Text Assist card is gone.
    expect(screen.queryByText(/Selection fixes \/ separate from dictation spend/)).not.toBeInTheDocument();
  });

  it("switches periods with the keyboard", async () => {
    const user = userEvent.setup();
    renderDashboard();
    const usage = await screen.findByRole("region", { name: "Your usage" });
    await within(usage).findByText(n(4321));
    const switcher = within(usage).getByRole("group", { name: "Statistics period" });
    expect(within(switcher).getByRole("button", { name: "7 days" })).toHaveAttribute("aria-pressed", "true");

    within(switcher).getByRole("button", { name: "7 days" }).focus();
    await user.keyboard("{Shift>}{Tab}{/Shift}");
    expect(within(switcher).getByRole("button", { name: "Today" })).toHaveFocus();
    await user.keyboard("{Enter}");

    expect(within(switcher).getByRole("button", { name: "Today" })).toHaveAttribute("aria-pressed", "true");
    expect(within(switcher).getByRole("button", { name: "7 days" })).toHaveAttribute("aria-pressed", "false");
    const dictation = within(usage).getByRole("region", { name: "Dictation" });
    expect(within(dictation).getByText("Words dictated").nextSibling).toHaveTextContent("120");
    const insights = screen.getByRole("region", { name: "Text Assist insights" });
    expect(insights).toHaveTextContent("3 runs today");

    await user.click(within(switcher).getByRole("button", { name: "All" }));
    expect(within(dictation).getByText("Words dictated").nextSibling).toHaveTextContent(n(4321));
    expect(screen.getByRole("region", { name: "Text Assist insights" })).toHaveTextContent("30 runs in total / 2 ensembles");
  });

  it("explains Text Assist outcomes, models, apps, presets and failures in plain words", async () => {
    renderDashboard();
    const insights = await screen.findByRole("region", { name: "Text Assist insights" });
    await within(insights).findByText("84%");
    expect(insights).toHaveTextContent("Apply rate (21 of 25 results)");
    expect(within(insights).getByRole("img", { name: /Applied 21, No change needed 1, Kept in Vellora 3, Cancelled 1, Failed 4/ })).toBeInTheDocument();
    expect(insights).toHaveTextContent("Median time to result1.1s");

    const models = within(insights).getByRole("table", { name: "Models" });
    const rows = within(models).getAllByRole("row");
    expect(rows[1]).toHaveTextContent("gpt-6-luna");
    expect(rows[1]).toHaveTextContent("OpenAI / effort: None");
    expect(rows[1]).toHaveTextContent("$0.03");
    expect(rows[2]).toHaveTextContent("ChatGPT plan");
    expect(rows[2]).toHaveTextContent("Plan / none");

    const apps = within(insights).getByRole("list", { name: "Top apps" });
    expect(apps).toHaveTextContent("code.exe");
    expect(apps).toHaveTextContent("12 applied, 2 failed");
    const presets = within(insights).getByRole("list", { name: "Presets" });
    expect(presets).toHaveTextContent("Quick Polish");
    expect(presets).toHaveTextContent("Formal");
    expect(presets).toHaveTextContent("Ensemble: Polish");

    const failures = within(insights).getByRole("list", { name: "Why runs failed" });
    expect(failures).toHaveTextContent("Another app was using the clipboard");
    expect(failures).toHaveTextContent("Nothing was selected or the app didn't copy");
    expect(failures).toHaveTextContent("Weird new code");
    expect(within(insights).getByRole("list", { name: "Why results were kept, not pasted" })).toHaveTextContent("The model added commentary");
  });

  it("charts both usages with an accessible summary and table", async () => {
    const user = userEvent.setup();
    renderDashboard();
    const activity = await screen.findByRole("region", { name: "Activity" });
    const chart = await within(activity).findByRole("img", { name: /Last 7 days: 14 transcriptions and 30 Text Assist runs/ });
    expect(chart).toBeInTheDocument();
    expect(within(activity).getByRole("table", { name: "Daily activity, last 7 days" })).toBeInTheDocument();
    expect(within(activity).getAllByRole("row")).toHaveLength(8);
    await user.click(within(activity).getByRole("button", { name: "30 days" }));
    expect(within(activity).getByRole("table", { name: "Daily activity, last 30 days" })).toBeInTheDocument();
    expect(within(activity).getAllByRole("row")).toHaveLength(31);
  });

  it("shows an empty state for a fresh installation", async () => {
    vi.mocked(api.getUsageOverview).mockResolvedValue(emptyUsageOverview());
    renderDashboard([]);
    const usage = await screen.findByRole("region", { name: "Your usage" });
    expect(await within(usage).findByText(/Nothing recorded yet. Hold Ctrl \+ Win to dictate/)).toBeInTheDocument();
    expect(usage).toHaveTextContent("press Shift + Win to fix it");
    expect(screen.getByRole("region", { name: "Text Assist insights" })).toHaveTextContent("No Text Assist runs in the last 7 days.");
    expect(screen.getByRole("img", { name: "No activity in the last 7 days." })).toBeInTheDocument();
  });

  it("reports load errors and keeps the rest of the page", async () => {
    vi.mocked(api.getUsageOverview).mockRejectedValue(new Error("Database unavailable"));
    renderDashboard();
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load statistics: Error: Database unavailable.");
    expect(screen.getByRole("heading", { name: "Recent transcriptions" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "System status" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Activity" })).not.toBeInTheDocument();
  });

  it("refreshes when a transcript arrives or a Text Assist run is recorded", async () => {
    const view = renderDashboard([]);
    await waitFor(() => expect(api.getUsageOverview).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(runRecorded).not.toBeNull());

    view.rerender(<DashboardView settings={settings()} hasKey snapshot={{ state: "idle" }} history={[transcript()]} health={health()} diagnostics={[]} />);
    await waitFor(() => expect(api.getUsageOverview).toHaveBeenCalledTimes(2));

    await act(async () => { runRecorded?.("run-9"); });
    await waitFor(() => expect(api.getUsageOverview).toHaveBeenCalledTimes(3));

    view.unmount();
    await waitFor(() => expect(runRecorded).toBeNull());
  });
});

describe("usage helpers", () => {
  it("formats time saved and reasons", () => {
    expect(formatMinutesSaved(0)).toBe("0 min");
    expect(formatMinutesSaved(0.4)).toBe("<1 min");
    expect(formatMinutesSaved(59.6)).toBe("1 h 00 min");
    expect(formatMinutesSaved(125)).toBe("2 h 05 min");
    expect(reasonLabel("no_selection")).toBe("Nothing was selected or the app didn't copy");
    expect(reasonLabel("password_field")).toBe("Password fields are skipped");
    expect(reasonLabel("brand_new_reason")).toBe("Brand new reason");
  });
});
