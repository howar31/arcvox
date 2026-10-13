// Compares two texts line by line, to show what a replacement would change.

export type LineChange = { kind: "keep" | "del" | "ins"; text: string };

/** The lines of `from` and `to` in reading order: kept, removed from `from`, or added in `to`. */
export function lineDiff(from: string, to: string): LineChange[] {
  const a = from.split("\n"), b = to.split("\n"), n = a.length, m = b.length;
  // Longest common subsequence, filled from the end.
  const table = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) table[i][j] = a[i] === b[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1]);
  }
  const out: LineChange[] = [];
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
