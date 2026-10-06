import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsView } from "../../src/views/SettingsView";
import { AppearanceView } from "../../src/views/AppearanceView";
import { useAppearance } from "../../src/lib/appearance";
import * as api from "../../src/lib/api";
import { settings } from "./fixtures";

vi.mock("../../src/lib/api", () => ({
  getStartAtLogin: vi.fn(),
  setStartAtLogin: vi.fn(),
  saveSettings: vi.fn(),
  saveOpenAiApiKey: vi.fn(),
  deleteOpenAiApiKey: vi.fn(),
}));

beforeEach(() => {
  vi.mocked(api.getStartAtLogin).mockResolvedValue(false);
});

describe("settings persistence", () => {
  it("keeps the saved model and reports a failed settings write", async () => {
    const user = userEvent.setup();
    const onSettingsChange = vi.fn().mockRejectedValue(new Error("Disk is read-only"));
    render(<SettingsView settings={settings()} hasKey onSettingsChange={onSettingsChange} onKeyStatusChange={vi.fn()} />);

    await user.selectOptions(screen.getByRole("combobox", { name: "Model" }), "gpt-4o-transcribe");

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save settings: Error: Disk is read-only");
    expect(onSettingsChange).toHaveBeenCalledWith({ sttModel: "gpt-4o-transcribe" });
    expect(screen.getByRole("combobox", { name: "Model" })).toHaveValue("gpt-4o-mini-transcribe");
    expect(screen.queryByText("Settings saved.")).not.toBeInTheDocument();
  });

  it("retains a failed API-key entry and never reports the key as saved", async () => {
    const user = userEvent.setup();
    const onKeyStatusChange = vi.fn();
    vi.mocked(api.saveOpenAiApiKey).mockRejectedValue(new Error("Credential Manager unavailable"));
    render(<SettingsView settings={settings()} hasKey={false} onSettingsChange={vi.fn()} onKeyStatusChange={onKeyStatusChange} />);

    const input = screen.getByLabelText("OpenAI API key");
    await user.type(input, "sk-test-only-not-a-real-key");
    await user.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save API key");
    expect(input).toHaveValue("sk-test-only-not-a-real-key");
    expect(onKeyStatusChange).not.toHaveBeenCalled();
    expect(screen.queryByText("API key saved.")).not.toBeInTheDocument();
  });

  it("keeps the startup switch at the confirmed value when Windows rejects a change", async () => {
    const user = userEvent.setup();
    vi.mocked(api.setStartAtLogin).mockRejectedValue(new Error("Access denied"));
    render(<SettingsView settings={settings()} hasKey onSettingsChange={vi.fn()} onKeyStatusChange={vi.fn()} />);
    const startup = screen.getByRole("checkbox", { name: "Start Vellora when I sign in to Windows" });
    await waitFor(() => expect(startup).toBeEnabled());
    await user.click(startup);

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not update Windows startup");
    expect(startup).not.toBeChecked();
  });
});

describe("appearance", () => {
  function AppearanceHarness() {
    const [saved, setSaved] = useState(settings());
    useAppearance(saved.appearance);
    async function changeSettings(patch: Partial<typeof saved>) {
      setSaved(await api.saveSettings({ ...saved, ...patch }));
    }
    return <AppearanceView settings={saved} onSettingsChange={changeSettings} />;
  }

  it("applies the confirmed saved theme to the document", async () => {
    const user = userEvent.setup();
    vi.mocked(api.saveSettings).mockImplementation(async (next) => next);
    render(<AppearanceHarness />);
    await user.click(screen.getByRole("radio", { name: /^Windows Classic/ }));

    await waitFor(() => expect(screen.getByRole("radio", { name: /^Windows Classic/ })).toBeChecked());
    expect(api.saveSettings).toHaveBeenCalledWith(expect.objectContaining({ appearance: "classic" }));
    expect(document.documentElement).toHaveAttribute("data-appearance", "classic");
  });

  it("preserves the previous appearance when saving fails", async () => {
    const user = userEvent.setup();
    vi.mocked(api.saveSettings).mockRejectedValue(new Error("Could not replace settings file"));
    render(<AppearanceHarness />);
    await user.click(screen.getByRole("radio", { name: /^Windows Classic/ }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save appearance");
    expect(screen.getByRole("radio", { name: /^Notebook/ })).toBeChecked();
    expect(document.documentElement).toHaveAttribute("data-appearance", "notebook");
  });
});
