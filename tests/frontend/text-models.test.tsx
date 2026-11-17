import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ModelsView } from "../../src/views/ModelsView";
import * as api from "../../src/lib/api";
import type { AppSettings, CompatEndpoint, ModelRef, TextAssistSettings } from "../../src/lib/types";
import { settings, textAssistOverview, textAssistSettings, textProviderKeys } from "./fixtures";

vi.mock("../../src/lib/api", () => ({
  getTextAssistOverview: vi.fn(),
  onTextAssistRunRecorded: vi.fn(),
  getTextProviderKeys: vi.fn(),
  saveTextProviderKey: vi.fn(),
  deleteTextProviderKey: vi.fn(),
}));

const luna: ModelRef = { provider: "openai", model: "gpt-6-luna", effort: "none" };
const ollama: CompatEndpoint = { id: "ollama", name: "Ollama", baseUrl: "http://127.0.0.1:11434/v1", models: ["llama3.2", "qwen3:8b"] };

beforeEach(() => {
  vi.mocked(api.getTextAssistOverview).mockResolvedValue(textAssistOverview());
  vi.mocked(api.onTextAssistRunRecorded).mockResolvedValue(() => {});
  vi.mocked(api.getTextProviderKeys).mockResolvedValue(textProviderKeys({ gemini: true }));
  vi.mocked(api.saveTextProviderKey).mockResolvedValue();
  vi.mocked(api.deleteTextProviderKey).mockResolvedValue();
});

/** Applies saved patches like App does, so the view re-renders with confirmed settings. */
function Harness({ initial, onChange }: { initial: AppSettings; onChange: (patch: Partial<AppSettings>) => Promise<void> }) {
  const [current, setCurrent] = useState(initial);
  return (
    <ModelsView settings={current} onSettingsChange={async (patch) => {
      await onChange(patch);
      setCurrent((value) => ({ ...value, ...patch }));
    }} />
  );
}

async function openTextModels(assist: Partial<TextAssistSettings> = {}, onChange = vi.fn().mockResolvedValue(undefined)) {
  const user = userEvent.setup();
  render(<Harness initial={settings({ textAssist: textAssistSettings(assist) })} onChange={onChange} />);
  expect(screen.getByRole("button", { name: /GPT-4o Transcribe Use for harder audio/ })).toBeInTheDocument();
  await user.click(screen.getByRole("tab", { name: "Text models" }));
  expect(screen.getByRole("tabpanel", { name: "Text models" })).toBeInTheDocument();
  await screen.findAllByText("Key saved");
  return { user, onChange };
}

function lastAssist(onChange: ReturnType<typeof vi.fn>) {
  return (onChange.mock.lastCall![0] as { textAssist: TextAssistSettings }).textAssist;
}

describe("Models tabs", () => {
  it("keeps transcription models in their own tab and supports arrow keys", async () => {
    const user = userEvent.setup();
    render(<Harness initial={settings()} onChange={vi.fn().mockResolvedValue(undefined)} />);
    expect(screen.getByRole("tab", { name: "Transcription" })).toHaveAttribute("aria-selected", "true");
    screen.getByRole("tab", { name: "Transcription" }).focus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Text models" })).toHaveFocus();
    expect(await screen.findByRole("heading", { name: "Provider keys" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /GPT-4o Transcribe/ })).not.toBeInTheDocument();
  });
});

describe("provider keys", () => {
  it("shows the status and privacy note per provider", async () => {
    await openTextModels();
    const openai = screen.getByRole("article", { name: "OpenAI" });
    expect(within(openai).getByText("Key saved")).toBeInTheDocument();
    expect(within(openai).getByText(/Settings, General, OpenAI key/)).toBeInTheDocument();
    expect(within(openai).getByText(/store:false/)).toBeInTheDocument();
    expect(within(openai).queryByLabelText(/API key/)).not.toBeInTheDocument();
    expect(within(screen.getByRole("article", { name: "Anthropic" })).getByText("No key saved")).toBeInTheDocument();
    const gemini = screen.getByRole("article", { name: "Google Gemini" });
    expect(within(gemini).getByText("Key saved")).toBeInTheDocument();
    expect(within(gemini).getByText(/Free tier: Google may use the text you send to improve its products\. Paid tier .* it does not\./)).toBeInTheDocument();
    expect(api.getTextProviderKeys).toHaveBeenCalledWith([]);
  });

  it("saves a key and refreshes the status without showing the key again", async () => {
    const { user } = await openTextModels();
    const anthropic = screen.getByRole("article", { name: "Anthropic" });
    expect(within(anthropic).queryByRole("button", { name: "Remove Anthropic key" })).not.toBeInTheDocument();
    vi.mocked(api.getTextProviderKeys).mockResolvedValue(textProviderKeys({ anthropic: true, gemini: true }));
    await user.type(within(anthropic).getByLabelText("Anthropic API key"), "  sk-ant-test  ");
    await user.click(within(anthropic).getByRole("button", { name: "Save Anthropic key" }));

    expect(api.saveTextProviderKey).toHaveBeenCalledWith("anthropic", null, "sk-ant-test");
    expect(await within(anthropic).findByRole("status")).toHaveTextContent("Anthropic key saved.");
    expect(await within(anthropic).findByText("Key saved")).toBeInTheDocument();
    expect(within(anthropic).getByLabelText("Replace Anthropic API key")).toHaveValue("");
    expect(within(anthropic).getByRole("button", { name: "Remove Anthropic key" })).toBeInTheDocument();
  });

  it("ignores an older key status response that arrives late", async () => {
    let resolveFirst!: (value: ReturnType<typeof textProviderKeys>) => void;
    vi.mocked(api.getTextProviderKeys)
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValue(textProviderKeys({ anthropic: true }));
    const user = userEvent.setup();
    render(<Harness initial={settings()} onChange={vi.fn().mockResolvedValue(undefined)} />);
    await user.click(screen.getByRole("tab", { name: "Text models" }));
    const anthropic = screen.getByRole("article", { name: "Anthropic" });
    expect(within(anthropic).getByText("Checking...")).toBeInTheDocument();
    await user.type(within(anthropic).getByLabelText("Anthropic API key"), "sk-ant-test{Enter}");
    expect(await within(anthropic).findByText("Key saved")).toBeInTheDocument();
    resolveFirst(textProviderKeys());
    await waitFor(() => expect(api.getTextProviderKeys).toHaveBeenCalledTimes(2));
    expect(within(anthropic).getByText("Key saved")).toBeInTheDocument();
  });

  it("keeps the draft and reports a failed save", async () => {
    vi.mocked(api.saveTextProviderKey).mockRejectedValue("Credential Manager is locked");
    const { user } = await openTextModels();
    const anthropic = screen.getByRole("article", { name: "Anthropic" });
    await user.type(within(anthropic).getByLabelText("Anthropic API key"), "sk-ant-test{Enter}");
    expect(await within(anthropic).findByRole("alert")).toHaveTextContent("Could not save the Anthropic key: Credential Manager is locked");
    expect(within(anthropic).getByLabelText("Anthropic API key")).toHaveValue("sk-ant-test");
    expect(within(anthropic).getByText("No key saved")).toBeInTheDocument();
  });

  it("removes a key only after confirmation", async () => {
    const { user } = await openTextModels();
    const gemini = screen.getByRole("article", { name: "Google Gemini" });
    await user.click(within(gemini).getByRole("button", { name: "Remove Google Gemini key" }));
    const confirm = within(gemini).getByRole("group", { name: "Confirm removing the Google Gemini key" });
    expect(within(confirm).getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.click(within(confirm).getByRole("button", { name: "Cancel" }));
    expect(api.deleteTextProviderKey).not.toHaveBeenCalled();

    vi.mocked(api.getTextProviderKeys).mockResolvedValue(textProviderKeys());
    await user.click(within(gemini).getByRole("button", { name: "Remove Google Gemini key" }));
    await user.click(within(gemini).getByRole("button", { name: "Remove key" }));
    expect(api.deleteTextProviderKey).toHaveBeenCalledWith("gemini", null);
    expect(await within(gemini).findByText("No key saved")).toBeInTheDocument();
    expect(within(gemini).getByRole("status")).toHaveTextContent("Google Gemini key removed.");
    expect(within(gemini).queryByRole("group")).not.toBeInTheDocument();
  });
});

describe("OpenAI-compatible endpoints", () => {
  it("validates and adds an endpoint with an optional key", async () => {
    const { user, onChange } = await openTextModels();
    expect(screen.getByText(/only fully offline option/)).toBeInTheDocument();
    expect(screen.getByText(/sends no reasoning effort/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add endpoint" }));
    const form = screen.getByRole("form", { name: "Add an endpoint" });
    const name = within(form).getByLabelText("Name");
    expect(name).toHaveFocus();

    await user.click(within(form).getByRole("button", { name: "Add endpoint" }));
    expect(onChange).not.toHaveBeenCalled();
    expect(name).toHaveAttribute("aria-invalid", "true");
    expect(name).toHaveAccessibleDescription("Enter a name.");

    await user.type(name, "LM Studio");
    expect(within(form).getByText("ID lm-studio, derived from the name.")).toBeInTheDocument();
    const url = within(form).getByLabelText("Base URL");
    await user.type(url, "http://192.168.1.5:1234/v1");
    expect(url).toHaveAccessibleDescription("Use HTTPS. Plain HTTP is only allowed for 127.0.0.1, localhost or [::1].");
    await user.clear(url);
    await user.type(url, "https://user:pw@example.com/v1");
    expect(url).toHaveAccessibleDescription(/Remove sign-in details/);
    await user.clear(url);
    await user.type(url, "http://localhost:1234/v1");
    const models = within(form).getByLabelText("Model IDs");
    await user.type(models, "qwen3, bad model!");
    expect(models).toHaveAccessibleDescription(/Not a valid model ID: model!/);
    await user.clear(models);
    await user.type(models, "qwen3{Enter}llama3.2");
    await user.type(within(form).getByLabelText("API key (optional)"), "local-key");
    await user.click(within(form).getByRole("button", { name: "Add endpoint" }));

    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(1));
    expect(lastAssist(onChange).openaiCompatible).toEqual([
      { id: "lm-studio", name: "LM Studio", baseUrl: "http://localhost:1234/v1", models: ["qwen3", "llama3.2"] },
    ]);
    expect(api.saveTextProviderKey).toHaveBeenCalledWith("openai_compatible", "lm-studio", "local-key");
    expect(await screen.findByText("LM Studio added.")).toBeInTheDocument();
    const list = screen.getByRole("list", { name: "OpenAI-compatible endpoints" });
    expect(within(list).getByText("On this computer")).toBeInTheDocument();
    expect(within(list).getByText("Models: qwen3, llama3.2")).toBeInTheDocument();
    await waitFor(() => expect(api.getTextProviderKeys).toHaveBeenLastCalledWith(["lm-studio"]));
  });

  it("rejects a duplicate endpoint name", async () => {
    const { user, onChange } = await openTextModels({ openaiCompatible: [ollama] });
    await user.click(screen.getByRole("button", { name: "Add endpoint" }));
    const form = screen.getByRole("form", { name: "Add an endpoint" });
    await user.type(within(form).getByLabelText("Name"), "ollama");
    await user.type(within(form).getByLabelText("Base URL"), "http://localhost:11434/v1");
    await user.click(within(form).getByRole("button", { name: "Add endpoint" }));
    expect(within(form).getByLabelText("Name")).toHaveAccessibleDescription("Another endpoint already uses this name.");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("edits an endpoint and drops favorites for models it no longer lists", async () => {
    const local: ModelRef = { provider: "openai_compatible", model: "llama3.2", endpoint: "ollama" };
    const { user, onChange } = await openTextModels({ openaiCompatible: [ollama], favoriteModels: [luna, local] });
    await user.click(screen.getByRole("button", { name: "Edit Ollama" }));
    const form = screen.getByRole("form", { name: "Edit Ollama" });
    expect(within(form).getByText("ID ollama (fixed).")).toBeInTheDocument();
    expect(within(form).queryByLabelText("API key (optional)")).not.toBeInTheDocument();
    const models = within(form).getByLabelText("Model IDs");
    expect(models).toHaveValue("llama3.2\nqwen3:8b");
    await user.clear(models);
    await user.type(models, "qwen3:8b");
    await user.clear(within(form).getByLabelText("Name"));
    await user.type(within(form).getByLabelText("Name"), "Ollama local");
    await user.click(within(form).getByRole("button", { name: "Save endpoint" }));

    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(1));
    expect(lastAssist(onChange)).toMatchObject({
      openaiCompatible: [{ ...ollama, name: "Ollama local", models: ["qwen3:8b"] }],
      favoriteModels: [luna],
    });
    expect(await screen.findByText("Ollama local updated. 1 favorite for removed models was removed.")).toBeInTheDocument();
  });

  it("removes an endpoint with its key after confirmation", async () => {
    vi.mocked(api.getTextProviderKeys).mockResolvedValue(textProviderKeys({ compat: { ollama: true } }));
    const local: ModelRef = { provider: "openai_compatible", model: "llama3.2", endpoint: "ollama" };
    const { user, onChange } = await openTextModels({ openaiCompatible: [ollama], favoriteModels: [luna, local] });
    await user.click(screen.getByRole("button", { name: "Remove Ollama" }));
    const confirm = screen.getByRole("group", { name: "Confirm removing Ollama" });
    await user.click(within(confirm).getByRole("button", { name: "Cancel" }));
    expect(onChange).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Remove Ollama" }));
    await user.click(screen.getByRole("button", { name: "Remove endpoint" }));
    await waitFor(() => expect(api.deleteTextProviderKey).toHaveBeenCalledWith("openai_compatible", "ollama"));
    expect(lastAssist(onChange)).toMatchObject({ openaiCompatible: [], favoriteModels: [luna] });
    expect(screen.queryByRole("list", { name: "OpenAI-compatible endpoints" })).not.toBeInTheDocument();
    expect(screen.getByText("Ollama removed.")).toBeInTheDocument();
  });

  it("manages an endpoint key from the list", async () => {
    vi.mocked(api.getTextProviderKeys).mockResolvedValue(textProviderKeys({ compat: { ollama: false } }));
    const { user } = await openTextModels({ openaiCompatible: [ollama] });
    expect(screen.getByText("No key (optional)")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Ollama API key"), "k-123");
    await user.click(screen.getByRole("button", { name: "Save Ollama key" }));
    expect(api.saveTextProviderKey).toHaveBeenCalledWith("openai_compatible", "ollama", "k-123");
  });
});

describe("favorite models", () => {
  it("adds catalog models with only their efforts and endpoint models without effort", async () => {
    const { user, onChange } = await openTextModels({ openaiCompatible: [ollama] });
    const form = screen.getByRole("form", { name: "Add a favorite model" });
    const model = within(form).getByLabelText("Model");
    const effort = within(form).getByLabelText("Effort");
    const groups = within(model).getAllByRole("group").map((group) => group.getAttribute("label"));
    expect(groups).toEqual(["OpenAI", "Anthropic", "Google Gemini", "Ollama (OpenAI-compatible)"]);
    expect(within(model).getByRole("option", { name: /^Claude Haiku 5\.5 .* needs key$/ })).toBeInTheDocument();

    await user.selectOptions(model, within(model).getByRole("option", { name: /^Gemini 3\.1 Flash-Lite/ }));
    expect(within(effort).getAllByRole("option").map((option) => option.getAttribute("value"))).toEqual(["minimal", "low", "medium", "high"]);
    expect(effort).toHaveValue("minimal");
    await user.selectOptions(effort, "high");
    await user.click(within(form).getByRole("button", { name: "Add favorite" }));
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(1));
    expect(lastAssist(onChange).favoriteModels).toEqual([luna, { provider: "gemini", model: "gemini-3.1-flash-lite", effort: "high" }]);

    await user.selectOptions(model, within(model).getByRole("option", { name: /^Claude Haiku/ }));
    expect(within(effort).getAllByRole("option").map((option) => option.getAttribute("value"))).toEqual(["none"]);
    expect(screen.getByText(/Anthropic has no saved key yet/)).toBeInTheDocument();

    await user.selectOptions(model, within(model).getByRole("option", { name: "llama3.2" }));
    expect(effort).toBeDisabled();
    expect(effort).toHaveDisplayValue("No effort sent");
    await user.click(within(form).getByRole("button", { name: "Add favorite" }));
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(2));
    expect(lastAssist(onChange).favoriteModels).toEqual([
      luna,
      { provider: "gemini", model: "gemini-3.1-flash-lite", effort: "high" },
      { provider: "openai_compatible", model: "llama3.2", endpoint: "ollama" },
    ]);
    const list = screen.getByRole("list", { name: "Favorite models" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(3);
    expect(within(list).getByText("No effort sent")).toBeInTheDocument();
  });

  it("rejects a duplicate favorite and stops at twelve", async () => {
    const { user, onChange } = await openTextModels();
    const form = screen.getByRole("form", { name: "Add a favorite model" });
    await user.selectOptions(within(form).getByLabelText("Model"), within(form).getByRole("option", { name: /^GPT-6 Luna/ }));
    await user.click(within(form).getByRole("button", { name: "Add favorite" }));
    expect(screen.getByRole("alert")).toHaveTextContent("This model and effort is already a favorite.");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("disables adding when twelve favorites exist", async () => {
    const many = Array.from({ length: 12 }, (_, index) => ({ provider: "openai", model: `m${index}` }) as ModelRef);
    await openTextModels({ favoriteModels: many });
    expect(screen.getByRole("button", { name: "Add favorite" })).toBeDisabled();
    expect(screen.getByText("You have 12 favorites, the maximum. Remove one to add another.")).toBeInTheDocument();
  });

  it("reorders, changes efforts and removes favorites", async () => {
    const haiku: ModelRef = { provider: "anthropic", model: "claude-haiku-5-5", effort: "none" };
    const gemini: ModelRef = { provider: "gemini", model: "gemini-3.1-flash-lite", effort: "low" };
    const { user, onChange } = await openTextModels({ favoriteModels: [luna, haiku, gemini], modelColors: { "anthropic/claude-haiku-5-5": "#000000" } });
    const list = screen.getByRole("list", { name: "Favorite models" });
    expect(within(list).getAllByRole("listitem")[1]).toHaveTextContent("needs key");
    expect(within(list).getByRole("button", { name: "Move GPT-6 Luna, None up" })).toBeDisabled();
    expect(within(list).getByRole("button", { name: "Move Gemini 3.1 Flash-Lite, Low down" })).toBeDisabled();

    await user.click(within(list).getByRole("button", { name: "Move Gemini 3.1 Flash-Lite, Low up" }));
    await waitFor(() => expect(lastAssist(onChange).favoriteModels).toEqual([luna, gemini, haiku]));
    await waitFor(() => expect(screen.getByRole("button", { name: "Move Gemini 3.1 Flash-Lite, Low up" })).toHaveFocus());
    expect(screen.getByText("Gemini 3.1 Flash-Lite, Low moved to position 2.")).toBeInTheDocument();

    const lunaEffort = screen.getByRole("combobox", { name: "Effort for GPT-6 Luna, None" });
    expect(within(lunaEffort).getAllByRole("option").map((option) => option.getAttribute("value"))).toEqual(["none", "low", "medium"]);
    await user.selectOptions(lunaEffort, "medium");
    await waitFor(() => expect(lastAssist(onChange).favoriteModels[0]).toEqual({ ...luna, effort: "medium" }));

    await user.click(screen.getByRole("button", { name: "Remove Claude Haiku 5.5, None from favorites" }));
    await waitFor(() => expect(lastAssist(onChange).favoriteModels).toEqual([{ ...luna, effort: "medium" }, gemini]));
    // The removed model's color override is pruned with it.
    expect(lastAssist(onChange).modelColors).toEqual({});
  });
});

describe("model colors", () => {
  it("overrides, validates and resets a model color", async () => {
    const { user, onChange } = await openTextModels();
    const colors = screen.getByRole("list", { name: "Model colors" });
    expect(within(colors).getAllByRole("listitem")).toHaveLength(1);
    expect(within(colors).getByText(/OpenAI · Quick Polish, favorite · default color/)).toBeInTheDocument();
    const hex = within(colors).getByLabelText("Hex color for GPT-6 Luna");
    expect(hex).toHaveValue("#5b8def");
    expect(within(colors).getByRole("button", { name: "Save color for GPT-6 Luna" })).toBeDisabled();

    await user.clear(hex);
    await user.type(hex, "teal{Enter}");
    expect(hex).toHaveAccessibleDescription("Enter a color as #rrggbb, for example #10a37f.");
    expect(onChange).not.toHaveBeenCalled();

    await user.clear(hex);
    await user.type(hex, "#ABCDEF");
    expect(within(colors).getByLabelText("Color picker for GPT-6 Luna")).toHaveValue("#abcdef");
    await user.click(within(colors).getByRole("button", { name: "Save color for GPT-6 Luna" }));
    await waitFor(() => expect(lastAssist(onChange).modelColors).toEqual({ "openai/gpt-6-luna": "#abcdef" }));
    expect(await screen.findByText("Color for GPT-6 Luna saved.")).toBeInTheDocument();
    expect(screen.getByLabelText("Hex color for GPT-6 Luna")).toHaveValue("#abcdef");

    await user.click(screen.getByRole("button", { name: "Reset color for GPT-6 Luna" }));
    await waitFor(() => expect(lastAssist(onChange).modelColors).toEqual({}));
    expect(screen.getByLabelText("Hex color for GPT-6 Luna")).toHaveValue("#5b8def");
    expect(screen.queryByRole("button", { name: "Reset color for GPT-6 Luna" })).not.toBeInTheDocument();
  });

  it("reports a failed save and keeps the saved color", async () => {
    const { user } = await openTextModels({}, vi.fn().mockRejectedValue(new Error("Disk is read-only")));
    const hex = screen.getByLabelText("Hex color for GPT-6 Luna");
    await user.clear(hex);
    await user.type(hex, "#123456{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save text model settings: Error: Disk is read-only");
    expect(screen.queryByRole("button", { name: "Reset color for GPT-6 Luna" })).not.toBeInTheDocument();
  });
});

describe("ensemble profiles", () => {
  function profilesSection() {
    return screen.getByRole("heading", { name: "Ensemble profiles" }).closest("section") as HTMLElement;
  }

  it("explains the Favorites fallback, validates and adds a profile", async () => {
    const { user, onChange } = await openTextModels();
    const section = profilesSection();
    expect(within(section).getByText(/uses one profile called "Favorites"/)).toBeInTheDocument();
    await user.click(within(section).getByRole("button", { name: "Add profile" }));
    const form = within(section).getByRole("form", { name: "Add an ensemble profile" });
    expect(within(form).getByLabelText("Name")).toHaveFocus();

    await user.click(within(form).getByRole("button", { name: "Add profile" }));
    expect(within(form).getByText("Enter a name.")).toBeInTheDocument();
    expect(within(form).getByRole("alert")).toHaveTextContent("Add at least one model.");
    expect(onChange).not.toHaveBeenCalled();

    await user.type(within(form).getByLabelText("Name"), "Deep academic");
    expect(within(form).getByText(/ID deep-academic, derived from the name\./)).toBeInTheDocument();
    const picker = within(form).getByRole("group", { name: "Add a model to the profile" });
    const model = within(picker).getByLabelText("Model");
    const effort = within(picker).getByLabelText("Effort");
    await user.selectOptions(model, within(model).getByRole("option", { name: /^GPT-6 Luna/ }));
    expect(within(effort).getAllByRole("option").map((option) => option.getAttribute("value"))).toEqual(["none", "low", "medium"]);
    await user.selectOptions(effort, "low");
    await user.click(within(picker).getByRole("button", { name: "Add model" }));
    // The same model again with another effort is allowed; the same effort twice is not.
    await user.selectOptions(effort, "medium");
    await user.click(within(picker).getByRole("button", { name: "Add model" }));
    await user.click(within(picker).getByRole("button", { name: "Add model" }));
    expect(within(form).getByRole("alert")).toHaveTextContent("This model and effort is already in the profile.");
    await user.selectOptions(model, within(model).getByRole("option", { name: /^Gemini 3\.1 Flash-Lite/ }));
    await user.click(within(picker).getByRole("button", { name: "Add model" }));
    await user.click(within(form).getByRole("button", { name: "Move Gemini 3.1 Flash-Lite, Medium up" }));
    await waitFor(() => expect(within(form).getByRole("button", { name: "Move Gemini 3.1 Flash-Lite, Medium up" })).toHaveFocus());

    await user.click(within(form).getByRole("button", { name: "Add profile" }));
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(1));
    expect(lastAssist(onChange).ensembleProfiles).toEqual([{
      id: "deep-academic",
      name: "Deep academic",
      members: [
        { ...luna, effort: "low" },
        { provider: "gemini", model: "gemini-3.1-flash-lite", effort: "medium" },
        { ...luna, effort: "medium" },
      ],
    }]);
    expect(await within(section).findByText("Deep academic added.")).toBeInTheDocument();
    const list = within(section).getByRole("list", { name: "Ensemble profiles" });
    expect(within(list).getByText("3 models")).toBeInTheDocument();
    expect(within(list).getByRole("list", { name: "Models in Deep academic" })).toHaveTextContent("GPT-6 Luna · Low");
  });

  it("edits a profile with a fixed id and removes it after confirmation", async () => {
    const haiku: ModelRef = { provider: "anthropic", model: "claude-haiku-5-5", effort: "none" };
    const { user, onChange } = await openTextModels({
      ensembleProfiles: [{ id: "fast", name: "Fast", members: [luna] }, { id: "other", name: "Other", members: [luna] }],
    });
    const section = profilesSection();
    expect(within(section).queryByText(/uses one profile called "Favorites"/)).not.toBeInTheDocument();
    await user.click(within(section).getByRole("button", { name: "Edit Fast" }));
    const form = within(section).getByRole("form", { name: "Edit Fast" });
    expect(within(form).getByText(/ID fast \(fixed\)\./)).toBeInTheDocument();
    const name = within(form).getByLabelText("Name");
    await user.clear(name);
    await user.type(name, "other");
    await user.click(within(form).getByRole("button", { name: "Remove GPT-6 Luna, None from the profile" }));
    await user.click(within(form).getByRole("button", { name: "Save profile" }));
    expect(within(form).getByText("Another profile already uses this name.")).toBeInTheDocument();
    expect(within(form).getByRole("alert")).toHaveTextContent("Add at least one model.");
    expect(onChange).not.toHaveBeenCalled();

    await user.clear(name);
    await user.type(name, "Quick");
    const picker = within(form).getByRole("group", { name: "Add a model to the profile" });
    const model = within(picker).getByLabelText("Model");
    await user.selectOptions(model, within(model).getByRole("option", { name: /^Claude Haiku/ }));
    await user.click(within(picker).getByRole("button", { name: "Add model" }));
    await user.selectOptions(model, within(model).getByRole("option", { name: /^GPT-6 Luna/ }));
    await user.click(within(picker).getByRole("button", { name: "Add model" }));
    await user.selectOptions(within(form).getByRole("combobox", { name: "Effort for GPT-6 Luna, None" }), "medium");
    await user.click(within(form).getByRole("button", { name: "Save profile" }));
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(1));
    expect(lastAssist(onChange).ensembleProfiles).toEqual([
      { id: "fast", name: "Quick", members: [haiku, { ...luna, effort: "medium" }] },
      { id: "other", name: "Other", members: [luna] },
    ]);

    await user.click(await within(section).findByRole("button", { name: "Remove Quick" }));
    const confirm = within(section).getByRole("group", { name: "Confirm removing Quick" });
    await user.click(within(confirm).getByRole("button", { name: "Remove profile" }));
    await waitFor(() => expect(onChange).toHaveBeenCalledTimes(2));
    expect(lastAssist(onChange).ensembleProfiles).toEqual([{ id: "other", name: "Other", members: [luna] }]);
    expect(await within(section).findByText("Quick removed.")).toBeInTheDocument();
  });

  it("disables adding at twenty profiles and saves the cost limit", async () => {
    const many = Array.from({ length: 20 }, (_, index) => ({ id: `p${index}`, name: `P${index}`, members: [luna] }));
    const { user, onChange } = await openTextModels({ ensembleProfiles: many });
    const section = profilesSection();
    expect(within(section).getByRole("button", { name: "Add profile" })).toBeDisabled();
    expect(within(section).getByText(/at most 20 profiles/)).toBeInTheDocument();

    const limit = within(section).getByLabelText("Cost limit per ensemble run (USD)");
    expect(limit).toHaveValue(0.05);
    await user.clear(limit);
    await user.type(limit, "7");
    expect(limit).toHaveAccessibleDescription("Enter an amount between $0 and $5.");
    expect(within(section).getByRole("button", { name: "Save limit" })).toBeDisabled();
    await user.clear(limit);
    await user.type(limit, "0.1{Enter}");
    await waitFor(() => expect(lastAssist(onChange).limits.ensembleMaxUsdPerRun).toBe(0.1));
    expect(lastAssist(onChange).limits.quickMaxChars).toBe(8000);
  });
});
