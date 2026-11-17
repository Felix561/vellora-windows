import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ModelsView } from "../../src/views/ModelsView";
import { SettingsView } from "../../src/views/SettingsView";
import * as api from "../../src/lib/api";
import {
  catalogGroupLabel,
  catalogOptionLabel,
  groupCatalogModels,
  needsBridge,
  needsKey,
  validateCliPath,
} from "../../src/lib/textAssist";
import type { AppSettings, CliBridges, TextAssistSettings, TextCliStatus } from "../../src/lib/types";
import { cliModels, cliStatus, settings, textAssistOverview, textAssistSettings, textCliStatus, textModel, textProviderKeys } from "./fixtures";

vi.mock("../../src/lib/api", () => ({
  getTextAssistOverview: vi.fn(),
  onTextAssistRunRecorded: vi.fn(),
  getTextProviderKeys: vi.fn(),
  saveTextProviderKey: vi.fn(),
  deleteTextProviderKey: vi.fn(),
  getTextCliStatus: vi.fn(),
  getStartAtLogin: vi.fn(),
  setStartAtLogin: vi.fn(),
  saveOpenAiApiKey: vi.fn(),
  deleteOpenAiApiKey: vi.fn(),
}));

const off: CliBridges = { codex: { enabled: false, path: null }, claude: { enabled: false, path: null } };
const codexOn: CliBridges = { ...off, codex: { enabled: true, path: null } };

beforeEach(() => {
  vi.mocked(api.getTextAssistOverview).mockResolvedValue(textAssistOverview({ models: [...textAssistOverview().models, ...cliModels()] }));
  vi.mocked(api.onTextAssistRunRecorded).mockResolvedValue(() => {});
  vi.mocked(api.getTextProviderKeys).mockResolvedValue(textProviderKeys());
  vi.mocked(api.getTextCliStatus).mockResolvedValue(textCliStatus());
  vi.mocked(api.getStartAtLogin).mockResolvedValue(false);
});

function Harness({ initial, onChange }: { initial: AppSettings; onChange: (patch: Partial<AppSettings>) => Promise<void> }) {
  const [current, setCurrent] = useState(initial);
  return (
    <ModelsView settings={current} onSettingsChange={async (patch) => {
      await onChange(patch);
      setCurrent((value) => ({ ...value, ...patch }));
    }} />
  );
}

async function openBridges(assist: Partial<TextAssistSettings> = {}, onChange = vi.fn().mockResolvedValue(undefined)) {
  const user = userEvent.setup();
  render(<Harness initial={settings({ textAssist: textAssistSettings(assist) })} onChange={onChange} />);
  await user.click(screen.getByRole("tab", { name: "Text models" }));
  const section = screen.getByRole("region", { name: "Subscription bridges (personal use)" });
  return { user, onChange, section };
}

function lastAssist(onChange: ReturnType<typeof vi.fn>) {
  return (onChange.mock.lastCall![0] as { textAssist: TextAssistSettings }).textAssist;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

describe("subscription bridge helpers", () => {
  it("validates custom CLI paths like the Rust settings rule", () => {
    expect(validateCliPath("   ")).toEqual({ path: null, error: null });
    expect(validateCliPath(" C:\\Tools\\codex.exe ")).toEqual({ path: "C:\\Tools\\codex.exe", error: null });
    expect(validateCliPath("\\\\server\\share\\claude.exe").error).toBeNull();
    expect(validateCliPath("\\\\?\\C:\\Tools\\codex.exe").error).toBeNull();
    expect(validateCliPath("codex.exe").error).toMatch(/full path/);
    expect(validateCliPath("Tools\\codex.exe").error).toMatch(/full path/);
    expect(validateCliPath("\\Tools\\codex.exe").error).toMatch(/full path/);
    expect(validateCliPath("\"C:\\Tools\\codex.exe\"").error).toMatch(/quotes/);
    expect(validateCliPath(`C:\\${"a".repeat(257)}`).error).toBeNull();
    expect(validateCliPath(`C:\\${"a".repeat(258)}`).error).toMatch(/260 characters/);
  });

  it("offers plan models only for enabled bridges and labels their billing", () => {
    const models = [textModel(), ...cliModels()];
    expect(groupCatalogModels(models).map((group) => group.provider)).toEqual(["openai"]);
    expect(groupCatalogModels(models, off).map((group) => group.provider)).toEqual(["openai"]);
    expect(groupCatalogModels(models, codexOn).map((group) => group.provider)).toEqual(["openai", "codex_cli"]);
    expect(groupCatalogModels(models, { ...codexOn, claude: { enabled: true, path: null } }).map((group) => group.provider))
      .toEqual(["openai", "codex_cli", "claude_cli"]);
    const [luna, sonnet] = cliModels();
    expect(catalogOptionLabel(luna)).toBe("GPT-6 Luna (ChatGPT plan) - $0 marginal");
    expect(catalogOptionLabel({ ...sonnet, label: "Sonnet" })).toBe("Sonnet - Claude plan, $0 marginal");
    expect(catalogGroupLabel("codex_cli")).toBe("ChatGPT plan via Codex CLI");
    expect(catalogGroupLabel("claude_cli")).toBe("Claude plan via Claude Code");
    expect(needsKey(luna, textProviderKeys({ openai: false }))).toBe(false);
    expect(needsBridge(luna, off)).toBe(true);
    expect(needsBridge(luna, codexOn)).toBe(false);
    expect(needsBridge(sonnet, codexOn)).toBe(true);
    expect(needsBridge(textModel(), off)).toBe(false);
  });
});

describe("subscription bridges section", () => {
  it("shows a loading state, then status, version and detected path, and refreshes on demand", async () => {
    const first = deferred<TextCliStatus>();
    vi.mocked(api.getTextCliStatus).mockReturnValueOnce(first.promise);
    const { user, section } = await openBridges();
    expect(within(section).getByText(/Vellora starts your own signed-in CLI in an empty temporary folder and sends only the selected text on standard input\. It never reads or stores your login\. Plan limits and the provider's terms apply; for personal use only\./)).toBeInTheDocument();
    expect(within(section).getByText(/Runs take several seconds; not recommended for Quick Polish\./)).toBeInTheDocument();
    expect(within(section).getByRole("button", { name: "Checking command-line tools" })).toBeDisabled();
    expect(section).toHaveAttribute("aria-busy", "true");

    first.resolve(textCliStatus({ claude: cliStatus({ installed: false, version: null, signedIn: null, planLogin: null, path: null }) }));
    const codex = await within(section).findByRole("article", { name: "ChatGPT plan via Codex CLI" });
    expect(within(codex).getByText("Ready")).toBeInTheDocument();
    expect(within(codex).getByText("Installed, version 0.169.0 · Signed in with your ChatGPT plan")).toBeInTheDocument();
    expect(within(codex).getByText("C:\\Tools\\codex.exe")).toBeInTheDocument();
    const claude = within(section).getByRole("article", { name: "Claude plan via Claude Code" });
    expect(within(claude).getByText("Not installed", { selector: ".ta-key-status" })).toBeInTheDocument();
    expect(within(claude).getByText("Not found on PATH or in the usual install folders.")).toBeInTheDocument();

    vi.mocked(api.getTextCliStatus).mockResolvedValueOnce(textCliStatus({ codex: cliStatus({ signedIn: false, planLogin: null }) }));
    await user.click(within(section).getByRole("button", { name: "Refresh command-line tool status" }));
    expect(await within(codex).findByText("Not signed in", { selector: ".ta-key-status" })).toBeInTheDocument();
    expect(api.getTextCliStatus).toHaveBeenCalledTimes(2);
  });

  it("warns that an API-key login bills the API instead of the plan", async () => {
    vi.mocked(api.getTextCliStatus).mockResolvedValue(textCliStatus({ claude: cliStatus({ planLogin: false }) }));
    const { section } = await openBridges();
    const claude = await within(section).findByRole("article", { name: "Claude plan via Claude Code" });
    expect(await within(claude).findByText("API-key login")).toBeInTheDocument();
    expect(within(claude).getByText(/signed in with an API key, so runs are billed to that API account, not your Claude plan/)).toBeInTheDocument();
    const codex = within(section).getByRole("article", { name: "ChatGPT plan via Codex CLI" });
    expect(within(codex).queryByText(/billed to that API account/)).not.toBeInTheDocument();
  });

  it("reports a failed status check", async () => {
    vi.mocked(api.getTextCliStatus).mockRejectedValue(new Error("boom"));
    const { section } = await openBridges();
    expect(await within(section).findByRole("alert")).toHaveTextContent("Could not check the command-line tools: Error: boom");
    expect(within(section).getByRole("button", { name: "Refresh command-line tool status" })).toBeEnabled();
  });

  it("enables a bridge and warns when the tool is not signed in", async () => {
    vi.mocked(api.getTextCliStatus).mockResolvedValue(textCliStatus({ codex: cliStatus({ signedIn: false, planLogin: null }) }));
    const { user, onChange, section } = await openBridges();
    const codex = await within(section).findByRole("article", { name: "ChatGPT plan via Codex CLI" });
    await within(codex).findByText("Not signed in", { selector: ".ta-key-status" });
    const toggle = within(codex).getByRole("checkbox", { name: "Use ChatGPT plan models" });
    expect(toggle).not.toBeChecked();
    await user.click(toggle);
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(1));
    expect(lastAssist(onChange).cli).toEqual(codexOn);
    expect(await within(section).findByText("ChatGPT plan via Codex CLI turned on.")).toBeInTheDocument();
    expect(toggle).toBeChecked();
    expect(toggle).toHaveAccessibleDescription(/Codex CLI is not signed in\. Run codex login in a terminal/);

    await user.click(toggle);
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(2));
    expect(lastAssist(onChange).cli).toEqual(off);
  });

  it("validates, saves and clears a custom path, then checks the status again", async () => {
    const { user, onChange, section } = await openBridges({ cli: { ...off, claude: { enabled: true, path: null } } });
    const claude = await within(section).findByRole("article", { name: "Claude plan via Claude Code" });
    await within(claude).findByText("Ready");
    const input = within(claude).getByRole("textbox", { name: "Custom path (optional)" });
    const saveButton = within(claude).getByRole("button", { name: "Save Claude Code path" });
    expect(saveButton).toBeDisabled();

    await user.type(input, "claude.exe");
    await user.click(saveButton);
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input).toHaveAccessibleDescription(/Enter a full path/);
    expect(input).toHaveFocus();
    expect(onChange).not.toHaveBeenCalled();

    await user.clear(input);
    await user.type(input, "D:\\Apps\\claude.exe");
    expect(input).not.toHaveAttribute("aria-invalid");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(1));
    expect(lastAssist(onChange).cli).toEqual({ ...off, claude: { enabled: true, path: "D:\\Apps\\claude.exe" } });
    expect(await within(section).findByText("Claude Code path saved.")).toBeInTheDocument();
    await waitFor(() => expect(api.getTextCliStatus).toHaveBeenCalledTimes(2));

    await user.clear(input);
    await user.click(saveButton);
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(2));
    expect(lastAssist(onChange).cli.claude).toEqual({ enabled: true, path: null });
    expect(await within(section).findByText("Claude Code path cleared; Vellora finds it on PATH.")).toBeInTheDocument();
  });
});

describe("plan models in the pickers", () => {
  it("offers plan models for favorites and ensembles only while their bridge is on", async () => {
    const { user, onChange, section } = await openBridges();
    await within(section).findAllByText("Ready");
    const favorites = screen.getByRole("form", { name: "Add a favorite model" });
    const picker = within(favorites).getByLabelText("Model");
    expect(within(picker).queryByRole("option", { name: /ChatGPT plan|Claude plan/ })).not.toBeInTheDocument();

    await user.click(within(section).getByRole("checkbox", { name: "Use ChatGPT plan models" }));
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(1));
    const groups = within(picker).getAllByRole("group").map((group) => group.getAttribute("label"));
    expect(groups).toContain("ChatGPT plan via Codex CLI");
    expect(groups).not.toContain("Claude plan via Claude Code");
    const option = within(picker).getByRole("option", { name: "GPT-6 Luna (ChatGPT plan) - $0 marginal" });
    await user.selectOptions(picker, option);
    expect(screen.queryByText(/has no saved key yet/)).not.toBeInTheDocument();
    await user.click(within(favorites).getByRole("button", { name: "Add favorite" }));
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(2));
    expect(lastAssist(onChange).favoriteModels.at(-1)).toEqual({ provider: "codex_cli", model: "gpt-6-luna", effort: "medium" });
    const list = screen.getByRole("list", { name: "Favorite models" });
    expect(within(list).getByText(/ChatGPT plan · \$0 marginal/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Add profile" }));
    const memberPicker = within(screen.getByRole("form", { name: /profile/i })).getAllByLabelText("Model")[0];
    expect(within(memberPicker).getByRole("option", { name: "GPT-6 Luna (ChatGPT plan) - $0 marginal" })).toBeInTheDocument();
    expect(within(memberPicker).queryByRole("option", { name: /Claude plan/ })).not.toBeInTheDocument();

    await user.click(within(section).getByRole("checkbox", { name: "Use ChatGPT plan models" }));
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(3));
    expect(within(picker).queryByRole("option", { name: /ChatGPT plan/ })).not.toBeInTheDocument();
    expect(within(list).getByText(/bridge off/)).toBeInTheDocument();
  });

  it("offers plan models in the Quick Polish picker only when enabled, with the latency note", async () => {
    const user = userEvent.setup();
    const onSettingsChange = vi.fn().mockResolvedValue(undefined);
    const view = render(<SettingsView settings={settings()} hasKey onSettingsChange={onSettingsChange} onKeyStatusChange={vi.fn()} />);
    await user.click(screen.getByRole("tab", { name: "Text Assist" }));
    await waitFor(() => expect(screen.getByRole("combobox", { name: "Model" })).toBeEnabled());
    expect(within(screen.getByRole("combobox", { name: "Model" })).queryByRole("option", { name: /Claude plan/ })).not.toBeInTheDocument();

    const claudeOn = textAssistSettings({ cli: { ...off, claude: { enabled: true, path: null } } });
    view.rerender(<SettingsView settings={settings({ textAssist: claudeOn })} hasKey onSettingsChange={onSettingsChange} onKeyStatusChange={vi.fn()} />);
    const picker = screen.getByRole("combobox", { name: "Model" });
    await user.selectOptions(picker, within(picker).getByRole("option", { name: "Claude Sonnet (Claude plan) - $0 marginal" }));
    await waitFor(() => expect(onSettingsChange).toHaveBeenCalledTimes(1));
    const quickModel = (onSettingsChange.mock.lastCall![0] as { textAssist: TextAssistSettings }).textAssist.quickModel;
    expect(quickModel).toEqual({ provider: "claude_cli", model: "sonnet", effort: "medium" });

    view.rerender(<SettingsView settings={settings({ textAssist: { ...claudeOn, quickModel } })} hasKey onSettingsChange={onSettingsChange} onKeyStatusChange={vi.fn()} />);
    expect(screen.getByText(/take several seconds; not recommended for Quick Polish/)).toBeInTheDocument();
    expect(screen.getByText("Uses your Claude plan via Claude Code ($0 marginal)")).toBeInTheDocument();
    expect(screen.queryByText(/needs an API key/)).not.toBeInTheDocument();

    view.rerender(<SettingsView settings={settings({ textAssist: { ...claudeOn, cli: off, quickModel } })} hasKey onSettingsChange={onSettingsChange} onKeyStatusChange={vi.fn()} />);
    expect(screen.getByRole("combobox", { name: "Model" })).toHaveDisplayValue("Claude Sonnet (Claude plan) (subscription bridge off)");
    expect(screen.getByText(/that subscription bridge is off/)).toBeInTheDocument();
  });
});
