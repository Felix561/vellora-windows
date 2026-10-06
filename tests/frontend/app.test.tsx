import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "../../src/App";
import { SETUP_STORAGE_KEY } from "../../src/lib/help";
import { health, settings, stats } from "./fixtures";

// Mock the native boundary, leaving the app and its API wrappers connected.
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

let keyStatus: boolean | Error;

beforeEach(() => {
  keyStatus = true;
  vi.mocked(listen).mockResolvedValue(vi.fn());
  vi.mocked(invoke).mockImplementation(async (command) => {
    switch (command) {
      case "get_settings": return settings();
      case "has_openai_api_key":
        if (keyStatus instanceof Error) throw keyStatus;
        return keyStatus;
      case "get_dictation_snapshot": return { state: "idle" };
      case "list_transcripts": return [];
      case "get_app_health": return health();
      case "get_dashboard_stats": return stats();
      case "get_support_info": return { version: "0.2.18", platform: "windows", architecture: "x86_64", startupPhase: "ready", ready: true, singleInstance: true, keyConfigured: keyStatus === true, microphoneAvailable: true, dictationState: "idle" };
      case "get_start_at_login": return false;
      default: throw new Error(`Unexpected native command in regression test: ${command}`);
    }
  });
});

describe("setup entry and persistence", () => {
  it("leaves existing users on the Dashboard when a key is already saved", async () => {
    render(<App />);
    expect(await screen.findByRole("heading", { level: 1, name: "Dashboard" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Guided setup" })).not.toBeInTheDocument();
    expect(localStorage.getItem(SETUP_STORAGE_KEY)).toBeNull();
  });

  it("does not interpret an unavailable key status as a new installation", async () => {
    keyStatus = new Error("Credential Manager unavailable");
    render(<App />);
    expect(await screen.findByRole("heading", { level: 1, name: "Dashboard" })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Could not read API key status");
    expect(screen.queryByRole("region", { name: "Guided setup" })).not.toBeInTheDocument();
  });

  it("opens setup for a confirmed missing key, remembers skip and permits reopening", async () => {
    const user = userEvent.setup();
    keyStatus = false;
    const view = render(<App />);
    expect(await screen.findByRole("region", { name: "Guided setup" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Skip for now" }));
    expect(localStorage.getItem(SETUP_STORAGE_KEY)).toBe("skipped");
    expect(screen.getByRole("heading", { level: 1, name: "Dashboard" })).toBeInTheDocument();

    view.unmount();
    render(<App />);
    expect(await screen.findByRole("heading", { level: 1, name: "Dashboard" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Guided setup" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Help & About" }));
    await user.click(screen.getByRole("button", { name: "Open guided setup" }));
    expect(screen.getByRole("region", { name: "Guided setup" })).toBeInTheDocument();
  });

  it("remembers completed setup without recording or contacting OpenAI", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("heading", { level: 1, name: "Dashboard" });
    await user.click(screen.getByRole("button", { name: "Help & About" }));
    await user.click(screen.getByRole("button", { name: "Open guided setup" }));
    for (let index = 0; index < 3; index++) await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.click(screen.getByRole("button", { name: "Finish setup" }));

    expect(localStorage.getItem(SETUP_STORAGE_KEY)).toBe("complete");
    expect(screen.getByRole("heading", { level: 1, name: "Dashboard" })).toBeInTheDocument();
    expect(vi.mocked(invoke).mock.calls.every(([command]) => command.startsWith("get_") || ["has_openai_api_key", "list_transcripts"].includes(command))).toBe(true);
  });

  it("closes setup with an honest warning when its reminder cannot be stored", async () => {
    const user = userEvent.setup();
    keyStatus = false;
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("Storage unavailable"); });
    render(<App />);
    await screen.findByRole("region", { name: "Guided setup" });
    await user.click(screen.getByRole("button", { name: "Skip for now" }));

    expect(screen.getByRole("heading", { level: 1, name: "Dashboard" })).toBeInTheDocument();
    expect(screen.getByText(/reminder preference could not be saved/)).toBeInTheDocument();
    expect(localStorage.getItem(SETUP_STORAGE_KEY)).toBeNull();
  });
});
