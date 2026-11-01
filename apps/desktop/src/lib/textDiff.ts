// Compares the recognised text with the final text, to show what the processing changed.

export type Change = { kind: "keep" | "del" | "ins"; text: string };

/** A word or a number of Latin letters is one unit, a run of spaces is one unit, anything else is a unit a character. */
export function units(text: string): string[] {
  return text.match(/[A-Za-z0-9']+|\s+|./gu) ?? [];
}

/** The units of `from` and `to` in reading order: kept, removed from `from`, or added in `to`. */
export function diff(from: string, to: string): Change[] {
  const a = units(from), b = units(to), n = a.length, m = b.length;
  // Longest common subsequence, filled from the end.
  const table = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) table[i][j] = a[i] === b[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1]);
  }
  const out: Change[] = [];
  let i = 0, j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) { out.push({ kind: "keep", text: a[i] }); i++; j++; }
    else if (table[i + 1][j] >= table[i][j + 1]) out.push({ kind: "del", text: a[i++] });
    else out.push({ kind: "ins", text: b[j++] });
  }
  while (i < n) out.push({ kind: "del", text: a[i++] });
  while (j < m) out.push({ kind: "ins", text: b[j++] });
  return out;
}

/** The same changes with neighbouring units of a kind joined, for marking a passage rather than each character. */
export function runs(changes: Change[]): Change[] {
  const out: Change[] = [];
  for (const c of changes) {
    const last = out.at(-1);
    if (last?.kind === c.kind) last.text += c.text;
    else out.push({ ...c });
  }
  return out;
}

/** How many places differ: a removal next to an addition is one place. */
export function changeCount(changes: Change[]): number {
  return changes.filter((c, i) => c.kind !== "keep" && changes[i - 1]?.kind !== "del" && changes[i - 1]?.kind !== "ins").length;
}
