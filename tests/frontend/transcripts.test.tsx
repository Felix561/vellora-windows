import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HistoryView } from "../../src/views/HistoryView";
import { TranscriptActions } from "../../src/views/TranscriptActions";
import { DashboardView } from "../../src/views/DashboardView";
import * as api from "../../src/lib/api";
import { health, settings, stats, transcript } from "./fixtures";

vi.mock("../../src/lib/api", () => ({
  clearTranscripts: vi.fn(),
  copyTranscript: vi.fn(),
  pasteTranscript: vi.fn(),
  deleteTranscript: vi.fn(),
  getDashboardStats: vi.fn(),
}));

beforeEach(() => {
  vi.mocked(api.clearTranscripts).mockResolvedValue(1);
  vi.mocked(api.copyTranscript).mockResolvedValue();
  vi.mocked(api.pasteTranscript).mockResolvedValue();
  vi.mocked(api.deleteTranscript).mockResolvedValue(true);
  vi.mocked(api.getDashboardStats).mockResolvedValue(stats());
});

describe("history deletion", () => {
  it("does not delete any history until the user confirms", async () => {
    const user = userEvent.setup();
    const onRefresh = vi.fn().mockResolvedValue(undefined);
    render(<HistoryView items={[transcript()]} onRefresh={onRefresh} />);
    await user.click(screen.getByRole("button", { name: "Clear all history" }));

    const confirmation = screen.getByRole("group", { name: "Confirm clearing history" });
    expect(api.clearTranscripts).not.toHaveBeenCalled();
    expect(within(confirmation).getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.click(within(confirmation).getByRole("button", { name: "Cancel" }));
    expect(api.clearTranscripts).not.toHaveBeenCalled();
    expect(screen.queryByRole("group", { name: "Confirm clearing history" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Clear all history" }));
    await user.click(screen.getByRole("button", { name: "Delete all transcripts" }));
    await waitFor(() => expect(onRefresh).toHaveBeenCalledOnce());
    expect(api.clearTranscripts).toHaveBeenCalledOnce();
    expect(screen.getByRole("status")).toHaveTextContent("Deleted 1 saved transcript.");
  });

  it("retains the transcript and reports an individual deletion failure", async () => {
    const user = userEvent.setup();
    vi.mocked(api.deleteTranscript).mockRejectedValue(new Error("Database unavailable"));
    render(<HistoryView items={[transcript()]} onRefresh={vi.fn().mockResolvedValue(undefined)} />);
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(api.deleteTranscript).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Delete transcript" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not delete: Error: Database unavailable");
    expect(screen.getByText("A recoverable dictation.")).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Confirm transcript deletion" })).toBeInTheDocument();
  });
});

describe("transcript recovery", () => {
  it("offers manual copying of the latest result when history is disabled", async () => {
    const user = userEvent.setup();
    render(<DashboardView settings={settings({ saveHistory: false })} hasKey snapshot={{ state: "done", lastTranscript: transcript({ pasted: true }) }} history={[]} health={health()} diagnostics={[]} />);
    const heading = screen.getByRole("heading", { name: "Latest transcript" });
    const recovery = heading.closest("section");
    expect(recovery).not.toBeNull();
    expect(within(recovery!).getByText("A recoverable dictation.")).toBeInTheDocument();
    await user.click(within(recovery!).getByRole("button", { name: "Copy" }));

    expect(api.copyTranscript).toHaveBeenCalledWith("test-transcript");
    expect(await screen.findByText("Copied to clipboard.")).toBeInTheDocument();
    expect(screen.getByText(/History saving is off/)).toBeInTheDocument();
  });
});

describe("delayed paste", () => {
  it("waits three seconds and pastes only the chosen transcript", async () => {
    vi.useFakeTimers();
    render(<TranscriptActions item={transcript()} />);
    fireEvent.click(screen.getByRole("button", { name: "Paste in 3s" }));
    await act(async () => { vi.advanceTimersByTime(2999); });
    expect(api.pasteTranscript).not.toHaveBeenCalled();
    await act(async () => { vi.advanceTimersByTime(1); });
    expect(api.pasteTranscript).toHaveBeenCalledOnce();
    expect(api.pasteTranscript).toHaveBeenCalledWith("test-transcript");
  });

  it("cancels the previous paste when a different transcript is chosen", async () => {
    vi.useFakeTimers();
    render(<><TranscriptActions item={transcript({ id: "first" })} /><TranscriptActions item={transcript({ id: "second" })} /></>);
    const controls = screen.getAllByRole("button", { name: "Paste in 3s" });
    fireEvent.click(controls[0]);
    await act(async () => { vi.advanceTimersByTime(1000); });
    fireEvent.click(controls[1]);
    await act(async () => { vi.advanceTimersByTime(3000); });

    expect(api.pasteTranscript).toHaveBeenCalledOnce();
    expect(api.pasteTranscript).toHaveBeenCalledWith("second");
  });

  it("never pastes after the controls have been unmounted", async () => {
    vi.useFakeTimers();
    const view = render(<TranscriptActions item={transcript()} />);
    fireEvent.click(screen.getByRole("button", { name: "Paste in 3s" }));
    view.unmount();
    await act(async () => { vi.advanceTimersByTime(4000); });
    expect(api.pasteTranscript).not.toHaveBeenCalled();
  });

  it("cancels a countdown when the latest transcript is replaced and can paste the replacement", async () => {
    vi.useFakeTimers();
    const view = render(<TranscriptActions item={transcript({ id: "first" })} />);
    fireEvent.click(screen.getByRole("button", { name: "Paste in 3s" }));
    await act(async () => { vi.advanceTimersByTime(1000); });
    view.rerender(<TranscriptActions item={transcript({ id: "second" })} />);
    await act(async () => { vi.advanceTimersByTime(3000); });

    expect(api.pasteTranscript).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: /Cancel paste/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Paste in 3s" }));
    await act(async () => { vi.advanceTimersByTime(3000); });
    expect(api.pasteTranscript).toHaveBeenCalledOnce();
    expect(api.pasteTranscript).toHaveBeenCalledWith("second");
  });

  it("never pastes after the user cancels the countdown", async () => {
    vi.useFakeTimers();
    render(<TranscriptActions item={transcript()} />);
    fireEvent.click(screen.getByRole("button", { name: "Paste in 3s" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel paste (3s)" }));
    await act(async () => { vi.advanceTimersByTime(4000); });
    expect(api.pasteTranscript).not.toHaveBeenCalled();
  });
});
