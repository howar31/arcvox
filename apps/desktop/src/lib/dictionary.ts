import type { DictionaryEntry } from "./api";

/** Trims each term and alias and drops blank aliases; the engine treats a
 * term with an inner line break as a snippet, so stray edge newlines from the
 * text box must not reach it. */
export function trimEntries(entries: DictionaryEntry[]): DictionaryEntry[] {
  return entries.map((e) => ({
    ...e,
    term: e.term.trim(),
    aliases: e.aliases.map((a) => a.trim()).filter(Boolean),
  }));
}
