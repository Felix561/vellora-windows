import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "../../src/App";
import type { AppSettings, AppSnapshot, TranscriptHistoryItem } from "../../src/lib/types";
import { health, settings, stats, transcript } from "./fixtures";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

let savedSettings: AppSettings;
let savedHistory: TranscriptHistoryItem[];
let savedSnapshot: AppSnapshot;
let keyStatus: boolean | Error;
const handlers = new Map<string, EventCallback<unknown>>();

beforeEach(() => {
  savedSettings = settings();
  savedHistory = [];
  savedSnapshot = { state: "idle" };
  keyStatus = true;
  handlers.clear();
  vi.mocked(listen).mockImplementation(async (event, handler) => {
    handlers.set(event, handler);
    return () => { handlers.delete(event); };
  });
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    switch (command) {
      case "get_settings": return savedSettings;
      case "has_openai_api_key":
        if (keyStatus instanceof Error) throw keyStatus;
        return keyStatus;
      case "get_dictation_snapshot": return savedSnapshot;
      case "list_transcripts": return savedHistory;
      case "get_app_health": return health();
      case "get_dashboard_stats": return stats();
      case "get_start_at_login": return false;
      case "save_settings":
        savedSettings = (args as { settings: AppSettings }).settings;
        return savedSettings;
      case "save_openai_api_key": keyStatus = true; return;
      default: throw new Error(`Unexpected native command: ${command}`);
    }
  });
});

async function emit(event: string, payload?: unknown) {
  await act(async () => { handlers.get(event)?.({ event, id: 1, payload }); });
}

async function openApp() {
  render(<App />);
  await screen.findByRole("heading", { level: 1, name: "Dashboard" });
}

function delayNextHealthRead() {
  const held = deferred<ReturnType<typeof health>>();
  const normal = vi.mocked(invoke).getMockImplementation()!;
  let pending = true;
  vi.mocked(invoke).mockImplementation((command, args) => {
    if (command === "get_app_health" && pending) { pending = false; return held.promise; }
    return normal(command, args);
  });
  return held;
}

describe("refresh/event ordering", () => {
  it("keeps the newer transcript when refreshes finish in reverse order", async () => {
    await openApp();
    const held = delayNextHealthRead();
    savedHistory = [transcript({ id: "old", finalText: "Older sample." })];
    await emit("history-changed");
    const latest = transcript({ id: "new", finalText: "Newer sample.", pasted: true });
    savedHistory = [latest];
    savedSnapshot = { state: "done", lastTranscript: latest };
    await emit("transcript-created", latest);
    expect(await screen.findByText("Newer sample.")).toBeInTheDocument();

    await act(async () => { held.resolve(health()); });
    expect(screen.getByText("Newer sample.")).toBeInTheDocument();
    expect(screen.queryByText("Older sample.")).not.toBeInTheDocument();
  });

  it("does not resurrect deleted history or latest text after an older refresh", async () => {
    const removed = transcript({ id: "removed", finalText: "Delete this sample." });
    savedHistory = [removed];
    savedSnapshot = { state: "done", lastTranscript: removed };
    await openApp();
    const held = delayNextHealthRead();
    // A manual refresh can read old rows before native deletion completes.
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "History" }));
    await user.click(screen.getByRole("button", { name: "Refresh history" }));
    savedHistory = [];
    savedSnapshot = { state: "idle", lastTranscript: null };
    await emit("history-changed");
    await waitFor(() => expect(screen.queryByText("Delete this sample.")).not.toBeInTheDocument());

    await act(async () => { held.resolve(health()); });
    await user.click(screen.getByRole("button", { name: "Dashboard" }));
    expect(screen.queryByText("Delete this sample.")).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Latest transcript" })).not.toBeInTheDocument();
  });

  it("clears deleted latest text even when a state event arrives during its follow-up read", async () => {
    const removed = transcript({ finalText: "Removed latest sample." });
    savedSnapshot = { state: "done", lastTranscript: removed };
    savedHistory = [removed];
    await openApp();
    const held = delayNextHealthRead();
    savedHistory = [];
    savedSnapshot = { state: "idle", lastTranscript: null };
    await emit("history-changed");
    await emit("dictation-state-changed", "recording");
    await act(async () => { held.resolve(health()); });

    expect(screen.queryByText("Removed latest sample.")).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Latest transcript" })).not.toBeInTheDocument();
    const pipeline = screen.getByRole("heading", { name: "Pipeline" }).closest("section")!;
    expect(within(pipeline).getByText("Recording...")).toBeInTheDocument();
  });

  it("does not replace newer settings, recording or health events with an old read", async () => {
    await openApp();
    const held = delayNextHealthRead();
    await emit("history-changed");
    savedSettings = settings({ appearance: "classic", saveHistory: false });
    await emit("settings-changed", savedSettings);
    await emit("dictation-state-changed", "recording");
    await emit("app-health-changed", { ...health(), ready: false, startupPhase: "degraded" });
    await act(async () => { held.resolve(health()); });

    expect(document.documentElement).toHaveAttribute("data-appearance", "classic");
    expect(screen.getByText("degraded")).toBeInTheDocument();
    expect(screen.getByText("Backend starting / recording")).toBeInTheDocument();
  });

  it("ignores an old failed refresh after newer successful data", async () => {
    await openApp();
    const normal = vi.mocked(invoke).getMockImplementation()!;
    const held = deferred<TranscriptHistoryItem[]>();
    let pending = true;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "list_transcripts" && pending) { pending = false; return held.promise; }
      return normal(command, args);
    });
    await emit("history-changed");
    const latest = transcript({ id: "new", finalText: "Successful newer sample.", pasted: true });
    savedHistory = [latest];
    savedSnapshot = { state: "done", lastTranscript: latest };
    await emit("transcript-created", latest);
    await act(async () => { held.reject(new Error("Stale failed read")); });

    expect(screen.getByText("Successful newer sample.")).toBeInTheDocument();
    expect(screen.queryByText(/Stale failed read/)).not.toBeInTheDocument();
  });

  it("keeps key-save success when an earlier presence lookup fails", async () => {
    await openApp();
    const user = userEvent.setup();
    const held = deferred<boolean>();
    const normal = vi.mocked(invoke).getMockImplementation()!;
    let pending = true;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "has_openai_api_key" && pending) { pending = false; return held.promise; }
      return normal(command, args);
    });
    await emit("history-changed");
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.type(screen.getByLabelText("Replace OpenAI API key"), "test-only-placeholder");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("API key saved.");
    await act(async () => { held.reject(new Error("Old key read unavailable")); });

    expect(screen.getByText("API key saved")).toBeInTheDocument();
    expect(screen.queryByText("API key status unavailable")).not.toBeInTheDocument();
    expect(screen.queryByText(/Could not read API key status/)).not.toBeInTheDocument();
  });

  it("marks a subsequent failed key lookup unavailable and mirrors locked recording", async () => {
    await openApp();
    keyStatus = new Error("Credential Manager unavailable");
    await emit("history-changed");
    expect(await screen.findAllByText("API key status unavailable")).toHaveLength(2);
    expect(screen.queryByText("API key missing")).not.toBeInTheDocument();
    await emit("dictation-state-changed", "recording");
    await emit("overlay-state-changed", { state: "recording", recordingMode: "locked", level: 0.2 });
    expect(screen.getByText("Locked")).toBeInTheDocument();
  });
});

describe("settings edits across navigation", () => {
  it("serializes history-off and a model edit without restoring history", async () => {
    await openApp();
    const user = userEvent.setup();
    const first = deferred<AppSettings>();
    const normal = vi.mocked(invoke).getMockImplementation()!;
    const writes: AppSettings[] = [];
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command !== "save_settings") return normal(command, args);
      const next = (args as { settings: AppSettings }).settings;
      writes.push(next);
      if (writes.length === 1) return first.promise;
      savedSettings = next;
      return Promise.resolve(next);
    });
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("checkbox", { name: "Save transcript history on this computer" }));
    await user.click(screen.getByRole("button", { name: "AI Models" }));
    await user.click(screen.getByRole("button", { name: /GPT-4o Transcribe Use for harder audio/ }));
    expect(writes).toHaveLength(1);
    expect(writes[0].saveHistory).toBe(false);
    await act(async () => { first.resolve(writes[0]); });
    await waitFor(() => expect(writes).toHaveLength(2));
    expect(writes[1]).toMatchObject({ saveHistory: false, sttModel: "gpt-4o-transcribe" });
    await user.click(screen.getByRole("button", { name: "Settings" }));
    expect(screen.getByRole("checkbox", { name: "Save transcript history on this computer" })).not.toBeChecked();
    expect(screen.getByRole("combobox", { name: "Model" })).toHaveValue("gpt-4o-transcribe");
  });

  it("merges a queued theme edit into acknowledged privacy settings", async () => {
    await openApp();
    const user = userEvent.setup();
    const first = deferred<AppSettings>();
    const normal = vi.mocked(invoke).getMockImplementation()!;
    const writes: AppSettings[] = [];
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command !== "save_settings") return normal(command, args);
      const next = (args as { settings: AppSettings }).settings;
      writes.push(next);
      if (writes.length === 1) return first.promise;
      return Promise.resolve(next);
    });
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("checkbox", { name: "Save transcript history on this computer" }));
    await user.click(screen.getByRole("tab", { name: "Appearance" }));
    await user.click(screen.getByRole("radio", { name: /^Windows Classic/ }));
    expect(writes).toHaveLength(1);
    await act(async () => { first.resolve(writes[0]); });
    await waitFor(() => expect(writes).toHaveLength(2));

    expect(writes[1]).toMatchObject({ saveHistory: false, appearance: "classic" });
    expect(document.documentElement).toHaveAttribute("data-appearance", "classic");
  });

  it("reports a failed write after navigation and allows the next edit", async () => {
    await openApp();
    const user = userEvent.setup();
    const first = deferred<AppSettings>();
    const normal = vi.mocked(invoke).getMockImplementation()!;
    let writes = 0;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "save_settings" && ++writes === 1) return first.promise;
      return normal(command, args);
    });
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("checkbox", { name: "Save transcript history on this computer" }));
    await user.click(screen.getByRole("button", { name: "AI Models" }));
    await act(async () => { first.reject(new Error("Disk is read-only")); });
    expect(screen.getByRole("alert")).toHaveTextContent("Could not save settings: Error: Disk is read-only");
    await user.click(screen.getByRole("button", { name: /GPT-4o Transcribe Use for harder audio/ }));
    await screen.findByText("Transcription model saved.");

    expect(savedSettings).toMatchObject({ saveHistory: true, sttModel: "gpt-4o-transcribe" });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
