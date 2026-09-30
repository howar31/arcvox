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

/** Adds the aliases in typed or pasted text (separated by commas or spaces), skipping duplicates. */
export function addAliases(aliases: string[], text: string): string[] {
  const out = [...aliases];
  for (const a of text.split(/[,，\s]+/)) {
    const alias = a.trim();
    if (alias && !out.includes(alias)) out.push(alias);
  }
  return out;
}

/** A term with a line break inside it is a snippet: saying an alias inserts the whole text. */
export function isSnippet(term: string): boolean {
  return term.trim().includes("\n");
}

/** Drops entries that were added but never filled in. */
export function dropBlank(entries: DictionaryEntry[]): DictionaryEntry[] {
  return entries.filter((e) => e.term.trim() !== "" || e.aliases.some((a) => a.trim() !== ""));
}
