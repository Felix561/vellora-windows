export type DiffPart = { kind: "same" | "added" | "removed"; text: string };

/** LCS cells allowed before falling back to a plain before/after view. */
export const MAX_DIFF_CELLS = 250_000;

type Token = { word: string; text: string };

/** Splits into words, each carrying its trailing whitespace; leading whitespace is its own token. */
function tokenize(text: string): Token[] {
  const tokens: Token[] = [];
  const leading = /^\s+/.exec(text);
  if (leading) tokens.push({ word: "", text: leading[0] });
  for (const match of text.matchAll(/\S+\s*/g)) {
    tokens.push({ word: match[0].trimEnd(), text: match[0] });
  }
  return tokens;
}

function push(parts: DiffPart[], kind: DiffPart["kind"], text: string) {
  if (!text) return;
  const last = parts[parts.length - 1];
  if (last && last.kind === kind) last.text += text;
  else parts.push({ kind, text });
}

/**
 * Word-level diff (LCS over whitespace-separated words). Whitespace changes alone are not
 * reported. Returns `null` when the texts are too long to diff within `maxCells`, so callers
 * can show plain before/after text instead.
 */
export function wordDiff(before: string, after: string, maxCells = MAX_DIFF_CELLS): DiffPart[] | null {
  const a = tokenize(before);
  const b = tokenize(after);
  let start = 0;
  while (start < a.length && start < b.length && a[start].word === b[start].word) start += 1;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1].word === b[endB - 1].word) { endA -= 1; endB -= 1; }

  const n = endA - start;
  const m = endB - start;
  if ((n + 1) * (m + 1) > maxCells) return null;

  const parts: DiffPart[] = [];
  for (let i = 0; i < start; i += 1) push(parts, "same", b[i].text);

  // lengths[i][j] = LCS length of a[start+i..endA) and b[start+j..endB).
  const width = m + 1;
  const lengths = new Uint32Array((n + 1) * width);
  for (let i = n - 1; i >= 0; i -= 1) {
    for (let j = m - 1; j >= 0; j -= 1) {
      lengths[i * width + j] = a[start + i].word === b[start + j].word
        ? lengths[(i + 1) * width + j + 1] + 1
        : Math.max(lengths[(i + 1) * width + j], lengths[i * width + j + 1]);
    }
  }
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[start + i].word === b[start + j].word) {
      push(parts, "same", b[start + j].text);
      i += 1;
      j += 1;
    } else if (i < n && (j === m || lengths[(i + 1) * width + j] >= lengths[i * width + j + 1])) {
      // Removals first, so a replaced word reads "old" then "new".
      push(parts, "removed", a[start + i].text);
      i += 1;
    } else {
      push(parts, "added", b[start + j].text);
      j += 1;
    }
  }

  for (let k = endB; k < b.length; k += 1) push(parts, "same", b[k].text);
  return parts;
}

/** Number of changed regions (each run of removed/added words counts once). */
export function countChanges(parts: DiffPart[]): number {
  let count = 0;
  let inChange = false;
  for (const part of parts) {
    if (part.kind === "same") inChange = false;
    else if (!inChange) { count += 1; inChange = true; }
  }
  return count;
}
