import { describe, expect, it } from "vitest";
import {
  MAX_FAVORITES,
  addFavorite,
  colorKey,
  endpointIdFromName,
  endpointsPatch,
  favoriteName,
  groupCatalogModels,
  isEndpointId,
  isHexColor,
  isLocalBaseUrl,
  isModelId,
  modelColorFor,
  modelOptionValue,
  modelRef,
  moveFavorite,
  needsKey,
  parseModelList,
  parseModelOptionValue,
  pruneModelColors,
  removeFavorite,
  resetModelColor,
  setFavoriteEffort,
  setModelColor,
  validateBaseUrl,
  validateEndpointDraft,
} from "../../src/lib/textAssist";
import type { CompatEndpoint, ModelRef } from "../../src/lib/types";
import { textAssistOverview, textAssistSettings, textModel, textProviderKeys } from "./fixtures";

const catalog = textAssistOverview().models;
const luna: ModelRef = { provider: "openai", model: "gpt-6-luna", effort: "none" };
const haiku: ModelRef = { provider: "anthropic", model: "claude-haiku-5-5", effort: "none" };
const ollama: CompatEndpoint = { id: "ollama", name: "Ollama", baseUrl: "http://127.0.0.1:11434/v1", models: ["llama3.2", "qwen3:8b"] };

describe("identifiers and validation", () => {
  it("matches the Rust model id, endpoint id and color rules", () => {
    expect(isModelId("openrouter/meta-llama:3.1_8b.q4")).toBe(true);
    expect(isModelId("")).toBe(false);
    expect(isModelId("a".repeat(129))).toBe(false);
    expect(isModelId("m;drop")).toBe(false);
    expect(isEndpointId("lm-studio_2")).toBe(true);
    expect(isEndpointId("Ollama")).toBe(false);
    expect(isEndpointId("a".repeat(41))).toBe(false);
    expect(isHexColor("#A1b2C3")).toBe(true);
    expect(isHexColor("#fff")).toBe(false);
    expect(isHexColor("red")).toBe(false);
  });

  it("derives unique endpoint ids from names", () => {
    expect(endpointIdFromName("LM Studio")).toBe("lm-studio");
    expect(endpointIdFromName("  Café Ollama!  ")).toBe("cafe-ollama");
    expect(endpointIdFromName("***")).toBe("endpoint");
    expect(endpointIdFromName("Ollama", ["ollama", "ollama-2"])).toBe("ollama-3");
    const long = endpointIdFromName("x".repeat(60), ["x".repeat(40)]);
    expect(long).toBe(`${"x".repeat(38)}-2`);
    expect(isEndpointId(long)).toBe(true);
  });

  it("allows HTTPS anywhere and plain HTTP only on loopback", () => {
    for (const url of ["https://api.groq.com/openai/v1", "http://127.0.0.1:11434/v1", "http://localhost:1234/v1", "http://[::1]:8080/v1"]) {
      expect(validateBaseUrl(url)).toBeNull();
    }
    expect(validateBaseUrl("")).toMatch(/Enter the endpoint's base URL/);
    expect(validateBaseUrl("example.com")).toMatch(/full URL/);
    expect(validateBaseUrl("ftp://example.com")).toMatch(/full URL/);
    expect(validateBaseUrl("http://example.com/v1")).toMatch(/Use HTTPS/);
    expect(validateBaseUrl("http://192.168.1.5:11434/v1")).toMatch(/Use HTTPS/);
    expect(validateBaseUrl("http://127.0.0.2/v1")).toMatch(/Use HTTPS/);
    expect(validateBaseUrl("https://user:secret@example.com/v1")).toMatch(/sign-in details/);
    expect(validateBaseUrl("https://example.com/v1?key=1")).toMatch(/query strings/);
    expect(validateBaseUrl("https://example.com/v1#x")).toMatch(/fragments/);
    expect(isLocalBaseUrl("http://localhost:1234/v1")).toBe(true);
    expect(isLocalBaseUrl("https://api.groq.com/openai/v1")).toBe(false);
  });

  it("parses model lists and validates endpoint drafts", () => {
    expect(parseModelList(" llama3.2, qwen3:8b\nllama3.2 ;\n")).toEqual(["llama3.2", "qwen3:8b"]);

    const added = validateEndpointDraft({ name: " LM Studio ", baseUrl: "http://localhost:1234/v1 ", models: "qwen3" }, [ollama], null);
    expect(added).toEqual({ endpoint: { id: "lm-studio", name: "LM Studio", baseUrl: "http://localhost:1234/v1", models: ["qwen3"] }, errors: null });

    const invalid = validateEndpointDraft({ name: "ollama", baseUrl: "http://example.com", models: "ok bad*id" }, [ollama], null);
    expect(invalid.errors).toEqual({
      name: "Another endpoint already uses this name.",
      baseUrl: expect.stringMatching(/Use HTTPS/),
      models: expect.stringMatching(/Not a valid model ID: bad\*id\./),
    });

    // Editing keeps the id and may keep its own name.
    const edited = validateEndpointDraft({ name: "Ollama", baseUrl: ollama.baseUrl, models: "llama3.2" }, [ollama], "ollama");
    expect(edited.endpoint).toEqual({ ...ollama, models: ["llama3.2"] });

    const tooMany = validateEndpointDraft({ name: "Big", baseUrl: "https://x.example/v1", models: Array.from({ length: 51 }, (_, i) => `m${i}`).join(",") }, [], null);
    expect(tooMany.errors?.models).toBe("Use at most 50 model IDs.");

    const ten = Array.from({ length: 10 }, (_, i) => ({ ...ollama, id: `e${i}`, name: `E${i}` }));
    expect(validateEndpointDraft({ name: "New", baseUrl: "https://x.example/v1", models: "" }, ten, null).errors?.name).toMatch(/at most 10 endpoints/);
  });
});

describe("favorites", () => {
  it("builds normalized references", () => {
    expect(modelRef("openai", "gpt-6-luna", "low", "ignored")).toEqual({ provider: "openai", model: "gpt-6-luna", effort: "low" });
    expect(modelRef("openai_compatible", "llama3.2", "high", "ollama")).toEqual({ provider: "openai_compatible", model: "llama3.2", endpoint: "ollama" });
  });

  it("adds, rejects duplicates and the 13th favorite", () => {
    expect(addFavorite([luna], haiku)).toEqual({ favorites: [luna, haiku], error: null });
    expect(addFavorite([luna], { ...luna }).error).toMatch(/already a favorite/);
    expect(addFavorite([luna], { ...luna, effort: "low" }).favorites).toHaveLength(2);
    const full = Array.from({ length: MAX_FAVORITES }, (_, i) => ({ ...luna, model: `m${i}` }));
    expect(addFavorite(full, haiku).error).toMatch(/at most 12/);
  });

  it("reorders and removes", () => {
    const list = [luna, haiku, { provider: "openai_compatible", model: "llama3.2", endpoint: "ollama" } as ModelRef];
    expect(moveFavorite(list, 2, -1)).toEqual([luna, list[2], haiku]);
    expect(moveFavorite(list, 0, -1)).toEqual(list);
    expect(moveFavorite(list, 2, 1)).toEqual(list);
    expect(removeFavorite(list, 1)).toEqual([luna, list[2]]);
  });

  it("only sets efforts the model offers", () => {
    const model = catalog.find((item) => item.id === "gpt-6-luna")!;
    expect(setFavoriteEffort([luna], 0, "medium", model)).toEqual({ favorites: [{ ...luna, effort: "medium" }], error: null });
    expect(setFavoriteEffort([luna], 0, "xhigh", model).error).toMatch(/does not offer/);
    expect(setFavoriteEffort([luna], 0, "low", null).error).toMatch(/does not offer/);
    expect(setFavoriteEffort([luna, { ...luna, effort: "low" }], 0, "low", model).error).toMatch(/already a favorite/);
  });

  it("names favorites and flags missing keys", () => {
    expect(favoriteName(luna, catalog, [])).toBe("GPT-6 Luna, None");
    expect(favoriteName({ provider: "openai_compatible", model: "llama3.2", endpoint: "ollama" }, catalog, [ollama])).toBe("llama3.2 on Ollama");
    const keys = textProviderKeys();
    expect(needsKey(haiku, keys)).toBe(true);
    expect(needsKey(luna, keys)).toBe(false);
    expect(needsKey({ provider: "openai_compatible" }, keys)).toBe(false);
    expect(needsKey(haiku, null)).toBe(false);
  });

  it("round-trips picker values and groups the catalog", () => {
    const value = modelOptionValue({ provider: "openai_compatible", model: "org/model:tag", endpoint: "ollama" });
    expect(parseModelOptionValue(value)).toEqual({ provider: "openai_compatible", endpoint: "ollama", model: "org/model:tag" });
    expect(parseModelOptionValue("gpt-6-luna")).toBeNull();
    const groups = groupCatalogModels([
      textModel({ id: "late", quickCandidate: false }),
      textModel({ provider: "gemini", id: "g" }),
      textModel({ id: "quick" }),
    ]);
    expect(groups.map((group) => [group.provider, group.models.map((model) => model.id)])).toEqual([
      ["openai", ["quick", "late"]],
      ["gemini", ["g"]],
    ]);
  });

  it("drops favorites of removed endpoints or models and prunes their colors", () => {
    const local: ModelRef = { provider: "openai_compatible", model: "qwen3:8b", endpoint: "ollama" };
    const assist = textAssistSettings({
      openaiCompatible: [ollama],
      favoriteModels: [luna, local],
      modelColors: { "openai_compatible/qwen3:8b": "#123456", "openai/gpt-6-luna": "#abcdef" },
    });
    expect(endpointsPatch(assist, [{ ...ollama, models: ["llama3.2"] }])).toEqual({
      openaiCompatible: [{ ...ollama, models: ["llama3.2"] }],
      favoriteModels: [luna],
      modelColors: { "openai/gpt-6-luna": "#abcdef" },
    });
    expect(endpointsPatch(assist, [ollama]).favoriteModels).toEqual([luna, local]);
    expect(endpointsPatch(assist, []).favoriteModels).toEqual([luna]);
  });
});

describe("model colors", () => {
  it("keys colors by provider and model and falls back to the catalog color", () => {
    expect(colorKey({ provider: "openai", model: "gpt-5.6-luna" })).toBe("openai/gpt-5.6-luna");
    expect(modelColorFor(haiku, {}, catalog)).toBe("#e9a07f");
    expect(modelColorFor({ provider: "anthropic", model: "unknown" }, {}, catalog)).toBe("#d97757");
    expect(modelColorFor({ provider: "openai_compatible", model: "x" }, {}, catalog)).toBe("#8a8f98");
    expect(modelColorFor(haiku, { "anthropic/claude-haiku-5-5": "#000000" }, catalog)).toBe("#000000");
  });

  it("sets, normalizes, resets and limits overrides", () => {
    const assist = textAssistSettings({ modelColors: { "openai/unused": "#111111" } });
    expect(setModelColor(assist, "openai/gpt-6-luna", "#ABCDEF", "#5b8def")).toEqual({ modelColors: { "openai/gpt-6-luna": "#abcdef" }, error: null });
    expect(setModelColor(assist, "openai/gpt-6-luna", "#5B8DEF", "#5b8def")).toEqual({ modelColors: {}, error: null });
    expect(setModelColor(assist, "openai/gpt-6-luna", "teal", "#5b8def").error).toMatch(/#rrggbb/);
    expect(resetModelColor({ "a/b": "#111111", "c/d": "#222222" }, "a/b")).toEqual({ "c/d": "#222222" });

    const favorites = Array.from({ length: 64 }, (_, i) => ({ provider: "openai", model: `m${i}` }) as ModelRef);
    const crowded = textAssistSettings({
      favoriteModels: favorites,
      modelColors: Object.fromEntries(favorites.map((ref) => [colorKey(ref), "#111111"])),
    });
    expect(pruneModelColors(crowded)).toEqual(crowded.modelColors);
    expect(setModelColor(crowded, "openai/gpt-6-luna", "#222222", "#5b8def").error).toMatch(/at most 64/);
  });
});
