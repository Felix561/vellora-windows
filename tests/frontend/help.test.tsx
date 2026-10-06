import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HelpView } from "../../src/views/HelpView";
import { formatSupportInfo, hasDismissedSetup, rememberSetup, SETUP_STORAGE_KEY } from "../../src/lib/help";
import type { AppSettings, SupportInfo } from "../../src/lib/types";
import * as api from "../../src/lib/api";
import { settings } from "./fixtures";

vi.mock("../../src/lib/api", () => ({ getSupportInfo: vi.fn(), openHelpLink: vi.fn() }));

function support(): SupportInfo {
  return {
    version: "0.2.18",
    platform: "windows",
    architecture: "x86_64",
    startupPhase: "ready",
    ready: true,
    singleInstance: true,
    keyConfigured: true,
    microphoneAvailable: true,
    dictationState: "idle",
  };
}

beforeEach(() => {
  vi.mocked(api.getSupportInfo).mockResolvedValue(support());
  vi.mocked(api.openHelpLink).mockResolvedValue();
});

describe("safe support information", () => {
  it("omits unexpected sensitive fields from backend data and settings", () => {
    const privateData = {
      apiKey: "private-api-key-marker",
      lastTranscript: { finalText: "private-dictation-marker" },
      error: "private-error-marker",
      deviceName: "private-device-marker",
      username: "private-username-marker",
      path: "C:\\Users\\private-path-marker",
    };
    const text = formatSupportInfo({ ...support(), ...privateData }, { ...settings(), ...privateData });
    for (const marker of ["private-api-key", "private-dictation", "private-error", "private-device", "private-username", "private-path"]) {
      expect(text).not.toContain(marker);
    }
    expect(JSON.parse(text)).toMatchObject({ app: "Vellora", version: "0.2.18", platform: "windows", transcriptionModel: "gpt-4o-mini-transcribe", keyConfigured: true });
  });

  it("rejects freeform values inserted into otherwise permitted fields", () => {
    const text = formatSupportInfo({
      ...support(),
      version: "private-version-marker",
      architecture: "private-architecture-marker",
      platform: "private-platform-marker",
      startupPhase: "private-status-marker",
      dictationState: "private-state-marker",
      keyConfigured: "private-key-marker",
    } as unknown as SupportInfo, {
      ...settings(),
      appearance: "private-theme-marker",
      sttModel: "private-model-marker",
      saveHistory: "private-history-marker",
    } as unknown as AppSettings);

    expect(text).not.toContain("private-");
    expect(JSON.parse(text)).toMatchObject({ version: "unknown", architecture: "unknown", platform: "unknown", startupPhase: "unknown", dictationState: "unknown", keyConfigured: "unknown", appearance: "unknown", transcriptionModel: "unknown", saveHistory: "unknown" });
  });

  it("shows copyable diagnostics even if clipboard access is unavailable", async () => {
    const user = userEvent.setup();
    vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(new Error("Clipboard unavailable"));
    render(<HelpView settings={settings()} onOpenSetup={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: "Copy support information" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Select and copy the support information below");
    const preview = screen.getByLabelText("Support information to review before sharing") as HTMLTextAreaElement;
    expect(preview.value).toContain('"platform": "windows"');
  });

  it("copies only sanitized information and waits for the user to open external help", async () => {
    const user = userEvent.setup();
    const copy = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
    vi.mocked(api.getSupportInfo).mockResolvedValue({ ...support(), apiKey: "private-api-key-marker" } as SupportInfo);
    render(<HelpView settings={settings()} onOpenSetup={vi.fn()} />);
    expect(api.openHelpLink).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Copy support information" }));

    expect(await screen.findByRole("status")).toHaveTextContent("Support information copied");
    expect(copy).toHaveBeenCalledOnce();
    expect(copy.mock.calls[0][0]).not.toContain("private-api-key-marker");
    await user.click(screen.getByRole("button", { name: "Full troubleshooting guide" }));
    expect(api.openHelpLink).toHaveBeenCalledWith("troubleshooting");
  });
});

describe("setup reminder preference", () => {
  it("recognizes only completed or skipped setup", () => {
    localStorage.setItem(SETUP_STORAGE_KEY, "unexpected");
    expect(hasDismissedSetup()).toBe(false);
    expect(rememberSetup("skipped")).toBe(true);
    expect(hasDismissedSetup()).toBe(true);
  });

  it("reports storage failure without preventing the app from continuing", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("Storage unavailable"); });
    expect(rememberSetup("complete")).toBe(false);
    expect(hasDismissedSetup()).toBe(false);
  });
});
