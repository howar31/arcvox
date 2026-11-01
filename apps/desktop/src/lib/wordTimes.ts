// Where each word of a transcript is in its recording.
import type { Stage } from "./api";

export interface Word { text: string; start_ms: number; end_ms: number }
export interface WordPiece { text: string; word: Word | null }

/** The word times the speech stage kept. */
export function wordsOf(stages: Stage[] | null | undefined): Word[] {
  const words = stages?.find((s) => s.kind === "asr")?.detail.words;
  return Array.isArray(words) ? (words as Word[]) : [];
}

/**
 * The transcript cut at its words, in order; what lies between them (marks, spaces) has no word. The words are the
 * engine's own, so they are found in `text`; `shown`, its script-converted form, is cut at the same places while it
 * has the same length.
 */
export function wordPieces(text: string, words: Word[], shown: string = text): WordPiece[] {
  const from = shown.length === text.length ? shown : text;
  const pieces: WordPiece[] = [];
  let at = 0;
  for (const word of words) {
    const found = text.indexOf(word.text, at);
    if (found < 0 || word.text === "") continue;
    if (found > at) pieces.push({ text: from.slice(at, found), word: null });
    at = found + word.text.length;
    pieces.push({ text: from.slice(found, at), word });
  }
  if (at < text.length) pieces.push({ text: from.slice(at), word: null });
  return pieces;
}
