import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TextAssistHistoryView } from "../../src/views/TextAssistHistoryView";
import * as api from "../../src/lib/api";
import { textAssistOverview, textAssistRun } from "./fixtures";

vi.mock("../../src/lib/api", () => ({
  getTextAssistOverview: vi.fn(),
  listTextAssistRuns: vi.fn(),
  deleteTextAssistRun: vi.fn(),
  clearTextAssistRuns: vi.fn(),
  copyTextAssistText: vi.fn(),
  onTextAssistRunRecorded: vi.fn(),
}));

beforeEach(() => {
  vi.mocked(api.getTextAssistOverview).mockResolvedValue(textAssistOverview());
  vi.mocked(api.onTextAssistRunRecorded).mockResolvedValue(() => undefined);
});

describe("TextAssistHistoryView failed attempts", () => {
  it("shows a text-free failure row as 'Nothing was captured' without text or copy action", async () => {
    vi.mocked(api.listTextAssistRuns).mockResolvedValue([
      textAssistRun({
        id: "failed-1",
        originalText: "",
        resultText: null,
        outcome: "failed",
        reasonCode: "no_selection",
        targetApp: "WINWORD.EXE",
        inputTokens: null,
        outputTokens: null,
        firstTokenMs: null,
        estimatedCostUsd: null,
      }),
    ]);
    const user = userEvent.setup();
    render(<TextAssistHistoryView saveHistory />);

    const list = await screen.findByRole("list", { name: "Text Assist runs" });
    expect(within(list).getByText("Nothing was captured")).toBeInTheDocument();
    await user.click(within(list).getByRole("button", { pressed: false }));

    const details = screen.getByRole("article", { name: "Run details" });
    expect(within(details).getByText(/could not read a selection in this app/)).toBeInTheDocument();
    expect(within(details).queryByRole("heading", { name: "Original" })).not.toBeInTheDocument();
    expect(within(details).queryByRole("button", { name: /Copy/ })).not.toBeInTheDocument();
    expect(within(details).getByRole("button", { name: /Delete/ })).toBeInTheDocument();
  });

  it("keeps the original and copy action for runs that captured text", async () => {
    vi.mocked(api.listTextAssistRuns).mockResolvedValue([textAssistRun({ outcome: "kept", reasonCode: "user_activity" })]);
    const user = userEvent.setup();
    render(<TextAssistHistoryView saveHistory />);

    const list = await screen.findByRole("list", { name: "Text Assist runs" });
    expect(within(list).queryByText("Nothing was captured")).not.toBeInTheDocument();
    await user.click(within(list).getByRole("button", { pressed: false }));
    const details = screen.getByRole("article", { name: "Run details" });
    expect(within(details).getByRole("heading", { name: "Original" })).toBeInTheDocument();
    expect(within(details).getByRole("button", { name: /Copy result/ })).toBeInTheDocument();
  });
});
