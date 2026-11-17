import { describe, expect, it } from "vitest";
import { countChanges, wordDiff } from "../../src/lib/wordDiff";

function render(parts: ReturnType<typeof wordDiff>) {
  return parts?.map((part) => (part.kind === "same" ? part.text : part.kind === "added" ? `{+${part.text}}` : `{-${part.text}}`)).join("");
}

describe("wordDiff", () => {
  it("reports no changes for identical text", () => {
    const parts = wordDiff("Hello world.", "Hello world.");
    expect(parts).toEqual([{ kind: "same", text: "Hello world." }]);
    expect(countChanges(parts!)).toBe(0);
  });

  it("marks a replaced word as removed then added", () => {
    const parts = wordDiff("Teh quick fox.", "The quick fox.");
    expect(parts).toEqual([
      { kind: "removed", text: "Teh " },
      { kind: "added", text: "The " },
      { kind: "same", text: "quick fox." },
    ]);
    expect(countChanges(parts!)).toBe(1);
  });

  it("finds insertions and deletions in the middle", () => {
    expect(render(wordDiff("a b c d", "a x c d e"))).toBe("a {-b }{+x }c d {+e}");
    expect(render(wordDiff("one two three", "one three"))).toBe("one {-two }three");
  });

  it("counts separate change regions", () => {
    const parts = wordDiff("Teh fox jump over teh dog", "The fox jumps over the dog")!;
    expect(countChanges(parts)).toBe(3);
  });

  it("ignores whitespace-only differences and keeps the result's whitespace", () => {
    const parts = wordDiff("a  b\nc", "a b c");
    expect(parts).toEqual([{ kind: "same", text: "a b c" }]);
  });

  it("handles empty inputs", () => {
    expect(wordDiff("", "")).toEqual([]);
    expect(wordDiff("", "new text")).toEqual([{ kind: "added", text: "new text" }]);
    expect(wordDiff("old text", "")).toEqual([{ kind: "removed", text: "old text" }]);
  });

  it("keeps leading whitespace", () => {
    expect(render(wordDiff("  hi there", "  hi here"))).toBe("  hi {-there}{+here}");
  });

  it("falls back to null when the changed middle is too large", () => {
    const before = Array.from({ length: 600 }, (_, i) => `w${i}`).join(" ");
    const after = Array.from({ length: 600 }, (_, i) => `v${i}`).join(" ");
    expect(wordDiff(before, after)).toBeNull();
    expect(wordDiff(before, after, 1_000_000)).not.toBeNull();
  });

  it("diffs long texts whose changes are small, thanks to prefix and suffix trimming", () => {
    const words = Array.from({ length: 5000 }, (_, i) => `w${i}`);
    const changed = [...words];
    changed[2500] = "changed";
    expect(render(wordDiff(words.join(" "), changed.join(" ")))).toContain("{-w2500 }{+changed }");
  });
});
