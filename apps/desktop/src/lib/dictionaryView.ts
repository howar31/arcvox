/** How the Dictionary page shows its entries: the view, the sort with its sections, and the search. Both
 * choices are kept on this computer only. */
import type { DictionaryEntry, TermHits, TermIndex } from "./api";
import { isSnippet } from "./dictionary";

export type View = "cards" | "list";
export type Sort = "added" | "updated" | "hits" | "strokes" | "zhuyin" | "pinyin" | "aliases" | "origin" | "kind";
export const VIEWS: View[] = ["cards", "list"];
export const SORTS: Sort[] = ["added", "updated", "hits", "strokes", "zhuyin", "pinyin", "aliases", "origin", "kind"];

const VIEW_KEY = "dictionary.view";
const SORT_KEY = "dictionary.sort";

export function storedView(): View {
  try {
    const v = localStorage.getItem(VIEW_KEY);
    // The two-pane view is gone; the list is the closest to it.
    if (v === "panes") return "list";
    return VIEWS.includes(v as View) ? (v as View) : "cards";
  } catch { return "cards"; }
}
export function rememberView(v: View): void {
  try { localStorage.setItem(VIEW_KEY, v); } catch { /* The choice is not remembered. */ }
}
export function storedSort(): Sort {
  try { const v = localStorage.getItem(SORT_KEY); return SORTS.includes(v as Sort) ? (v as Sort) : "added"; } catch { return "added"; }
}
export function rememberSort(v: Sort): void {
  try { localStorage.setItem(SORT_KEY, v); } catch { /* The choice is not remembered. */ }
}

/** Whether the term or an alias holds the query, ignoring case and spacing. */
export function matches(e: DictionaryEntry, query: string): boolean {
  const q = fold(query);
  if (!q) return true;
  return fold(e.term).includes(q) || e.aliases.some((a) => fold(a).includes(q));
}
const fold = (s: string) => s.replace(/\s+/g, "").toLowerCase();

/** What the first character of a term is: a Latin letter, a Han character, kana, or something else. */
export type Script = "latin" | "han" | "kana" | "other";
export function scriptOf(term: string): Script {
  const c = term.trim()[0] ?? "";
  if (/[A-Za-z]/.test(c)) return "latin";
  if (/[一-鿿]/.test(c)) return "han";
  if (/[぀-ヿ]/.test(c)) return "kana";
  return "other";
}

/** A section of the sorted list: its label key (or text) and its entries. */
export interface Section<T> { key: string; label: string; items: T[] }

export interface Labels {
  /** `{n}` strokes. */
  strokes: (n: number) => string;
  latin: string; kana: string; other: string; unknown: string;
  manual: string; suggestion: string; snippet: string; term: string;
}

/** Sorts the entries and cuts them into sections; `index` gives each term's keys in the same order, `hits` how often
 * each term was replaced. */
export function arrange<T extends DictionaryEntry>(items: T[], sort: Sort, index: (TermIndex | null)[], labels: Labels, hits: Map<string, TermHits> = new Map()): Section<T>[] {
  const keyed = items.map((e, i) => ({ e, i, idx: index[i] ?? null, script: scriptOf(e.term) }));
  const byTerm = (a: typeof keyed[number], b: typeof keyed[number]) => a.e.term.localeCompare(b.e.term, undefined, { sensitivity: "base", numeric: true });
  const one = (key: string, label: string, list: typeof keyed): Section<T>[] => (list.length ? [{ key, label, items: list.map((k) => k.e) }] : []);
  switch (sort) {
    case "added":
    case "updated": {
      const at = (k: typeof keyed[number]) => (sort === "added" ? k.e.added : (k.e.updated ?? k.e.added)) ?? 0;
      // Newest first; entries without a time keep their stored order after them.
      const sorted = [...keyed].sort((a, b) => at(b) - at(a) || a.i - b.i);
      return one("all", "", sorted);
    }
    case "hits": {
      // Most replaced first, the latest replacement breaking ties; terms never replaced come after, by name.
      const h = (k: typeof keyed[number]) => hits.get(k.e.term.trim());
      const sorted = [...keyed].sort((a, b) => (h(b)?.count ?? 0) - (h(a)?.count ?? 0) || (h(b)?.last ?? 0) - (h(a)?.last ?? 0) || byTerm(a, b));
      return one("all", "", sorted);
    }
    case "aliases": {
      const sorted = [...keyed].sort((a, b) => b.e.aliases.length - a.e.aliases.length || byTerm(a, b));
      return one("all", "", sorted);
    }
    case "origin": {
      const by = (o: "suggestion" | "manual") => keyed.filter((k) => (k.e.origin ?? "manual") === o).sort(byTerm);
      return [...one("suggestion", labels.suggestion, by("suggestion")), ...one("manual", labels.manual, by("manual"))];
    }
    case "kind": {
      const snippets = keyed.filter((k) => isSnippet(k.e.term)).sort(byTerm);
      const terms = keyed.filter((k) => !isSnippet(k.e.term)).sort(byTerm);
      return [...one("term", labels.term, terms), ...one("snippet", labels.snippet, snippets)];
    }
    case "strokes":
    case "zhuyin":
    case "pinyin": {
      const latin = keyed.filter((k) => k.script === "latin").sort(byTerm);
      const han = keyed.filter((k) => k.script === "han");
      const kana = keyed.filter((k) => k.script === "kana").sort(byTerm);
      const other = keyed.filter((k) => k.script === "other").sort(byTerm);
      const out: Section<T>[] = [];
      // Latin terms by their first letter, one section per letter; under "pinyin" the Han terms join them.
      const letters = new Map<string, typeof keyed>();
      const put = (letter: string, k: typeof keyed[number]) => { (letters.get(letter) ?? letters.set(letter, []).get(letter)!).push(k); };
      for (const k of latin) put(k.e.term.trim()[0].toUpperCase(), k);
      const hanLeft: typeof keyed = [];
      if (sort === "pinyin") {
        for (const k of han) { if (k.idx?.pinyin) put(k.idx.pinyin, k); else hanLeft.push(k); }
      }
      for (const letter of [...letters.keys()].sort()) out.push({ key: `L${letter}`, label: letter, items: letters.get(letter)!.sort(byTerm).map((k) => k.e) });
      if (sort === "strokes") {
        const groups = new Map<number, typeof keyed>();
        for (const k of han) { const n = k.idx?.strokes ?? 0; (groups.get(n) ?? groups.set(n, []).get(n)!).push(k); }
        for (const n of [...groups.keys()].sort((a, b) => a - b)) {
          if (n === 0) continue;
          out.push({ key: `S${n}`, label: labels.strokes(n), items: groups.get(n)!.sort(byTerm).map((k) => k.e) });
        }
        if (groups.has(0)) hanLeft.push(...groups.get(0)!);
      } else if (sort === "zhuyin") {
        const order = "ㄅㄆㄇㄈㄉㄊㄋㄌㄍㄎㄏㄐㄑㄒㄓㄔㄕㄖㄗㄘㄙㄧㄨㄩㄚㄛㄜㄞㄟㄠㄡㄢㄣㄤㄥㄦ";
        const groups = new Map<string, typeof keyed>();
        for (const k of han) { const z = k.idx?.zhuyin; if (z) (groups.get(z) ?? groups.set(z, []).get(z)!).push(k); else hanLeft.push(k); }
        for (const z of [...groups.keys()].sort((a, b) => order.indexOf(a) - order.indexOf(b))) out.push({ key: `Z${z}`, label: z, items: groups.get(z)!.sort(byTerm).map((k) => k.e) });
      }
      out.push(...one("unknown", labels.unknown, hanLeft.sort(byTerm)));
      out.push(...one("kana", labels.kana, kana));
      out.push(...one("other", labels.other, other));
      return out;
    }
  }
}
