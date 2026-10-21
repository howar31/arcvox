// What the quality test page derives from a run's rows and totals.
import type { Cer, EvalItem, EvalStage, EvalSummary, ReportHead } from "./api";

/** The error rate of a score as a percentage with one decimal, "0.0" for an empty expectation. */
export function rate(c: Cer): string {
  return (c.length > 0 ? (c.errors / c.length) * 100 : 0).toFixed(1);
}

/** The error rate as a number between 0 and 1. */
export function rateOf(c: Cer): number {
  return c.length > 0 ? c.errors / c.length : 0;
}

/** Seconds per row with one decimal, from a summary's total time. */
export function perRow(s: { ms_total: number; items: number }): string {
  return (s.items > 0 ? s.ms_total / s.items / 1000 : 0).toFixed(1);
}

/** The rows of one engine, worst first: failed rows, then by the word rate, then by the punctuation rate. */
export function worstFirst(items: EvalItem[], provider: string): EvalItem[] {
  return items
    .filter((it) => it.provider === provider)
    .sort((a, b) => {
      const fa = a.error ? 1 : 0, fb = b.error ? 1 : 0;
      if (fa !== fb) return fb - fa;
      const w = rateOf(b.text) - rateOf(a.text);
      if (w !== 0) return w;
      return rateOf(b.punctuation) - rateOf(a.punctuation);
    });
}

/** The engine ids a full-stage label names (`asr+llm`); any other label is one id. */
export function providersOf(label: string, stage: EvalStage): string[] {
  return stage === "full" ? label.split("+").filter((p) => p && p !== "none") : [label];
}

/** The engines of a run in the order they ran. */
export function providerOrder(summaries: EvalSummary[]): string[] {
  return summaries.map((s) => s.provider);
}

/** Reports of the same stage as `head`, itself left out: the ones it can be compared with. */
export function comparable(heads: ReportHead[], head: ReportHead | null): ReportHead[] {
  if (!head) return heads;
  return heads.filter((h) => h.stage === head.stage && h.file !== head.file);
}

/** Minutes and seconds of a duration in milliseconds, as `m:ss`. */
export function clock(ms: number): string {
  const s = Math.round(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** The share of rows done, 0 to 1. */
export function fraction(done: number, total: number): number {
  return total > 0 ? Math.min(1, done / total) : 0;
}
