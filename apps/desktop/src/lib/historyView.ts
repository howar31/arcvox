import type { Utterance } from "./api";
import { t } from "./i18n";

export type DayKey = "today" | "yesterday" | string;

function dayKey(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** Groups rows by local calendar day, newest day and newest row first.
 * Keys are "today", "yesterday", or the date as YYYY-MM-DD. */
export function groupByDay(rows: Utterance[], now: Date): { key: DayKey; rows: Utterance[] }[] {
  const today = dayKey(now);
  const yesterday = dayKey(new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1));
  const groups: { key: DayKey; rows: Utterance[] }[] = [];
  for (const row of [...rows].sort((a, b) => b.ts - a.ts)) {
    const date = dayKey(new Date(row.ts * 1000));
    const key = date === today ? "today" : date === yesterday ? "yesterday" : date;
    const last = groups[groups.length - 1];
    if (last && last.key === key) last.rows.push(row);
    else groups.push({ key, rows: [row] });
  }
  return groups;
}

/** Milliseconds as seconds with one decimal; null when the step did not run. */
export function seconds(ms: number | null): string | null {
  return ms === null ? null : (ms / 1000).toFixed(1);
}

/** Recognition plus rewriting, as seconds with one decimal; null when the rewrite did not run. */
export function totalSeconds(asrMs: number, llmMs: number | null): string | null {
  return llmMs === null ? null : seconds(asrMs + llmMs);
}

/** A clip length as minutes and seconds, m:ss. */
export function clipLength(ms: number): string {
  const total = Math.floor(ms / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

/** How the raw transcript relates to the final text: absent, changed by the rewrite, left as it was by the rewrite,
 * inserted because the rewrite failed, or the profile does not rewrite. */
/** The tag of a row processed again: once, or how many times. */
export function redoneTag(redone: number): string | null {
  if (redone <= 0) return null;
  return redone === 1 ? t("history.redone") : t("history.redone_times", { n: String(redone) });
}

export function rawState(u: Utterance): "none" | "changed" | "same" | "failed" | "off" {
  const raw = u.raw_text.trim();
  if (raw === "") return "none";
  if (raw !== u.final_text.trim()) return "changed";
  if (u.degraded) return "failed";
  return u.llm_ms === null ? "off" : "same";
}
