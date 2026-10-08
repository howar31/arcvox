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

/** The time window the History page shows. A range covers whole local days from `from` through `to`
 * (YYYY-MM-DD); on a single day `hourFrom` through `hourTo` (0 to 23, inclusive) narrow it to hours. */
export interface PeriodSel { kind: "all" | "today" | "yesterday" | "7d" | "30d" | "range"; from?: string; to?: string; hourFrom?: number; hourTo?: number }

/** Rows per page of the list. */
export const PAGE_SIZE = 100;

const toSec = (d: Date) => Math.floor(d.getTime() / 1000);
/** A day as YYYY-MM-DD, parsed to its parts. */
function dayParts(value: string | undefined): [number, number, number] | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value ?? "");
  return m ? [Number(m[1]), Number(m[2]) - 1, Number(m[3])] : null;
}
/** Local midnight of a day, offset by `days`. */
function dayStart(value: string | undefined, days = 0): number | null {
  const p = dayParts(value);
  return p ? toSec(new Date(p[0], p[1], p[2] + days)) : null;
}
/** The hours of a selection, ordered, when it is a single day with hours chosen. */
function hoursOf(p: PeriodSel): [number, number] | null {
  if (p.kind !== "range" || !p.from || p.from !== p.to || p.hourFrom === undefined) return null;
  const a = p.hourFrom, b = p.hourTo ?? p.hourFrom;
  return [Math.min(a, b), Math.max(a, b)];
}

/** The window of a selection, in unix seconds: `since` inclusive, `until` exclusive, null for open. */
export function periodRange(p: PeriodSel, now: Date): { since: number | null; until: number | null } {
  const midnight = (daysBack: number) => toSec(new Date(now.getFullYear(), now.getMonth(), now.getDate() - daysBack));
  switch (p.kind) {
    case "all": return { since: null, until: null };
    case "today": return { since: midnight(0), until: null };
    case "yesterday": return { since: midnight(1), until: midnight(0) };
    case "7d": return { since: midnight(6), until: null };
    case "30d": return { since: midnight(29), until: null };
    case "range": {
      const hours = hoursOf(p);
      const day = dayParts(p.from);
      if (hours && day) return { since: toSec(new Date(day[0], day[1], day[2], hours[0])), until: toSec(new Date(day[0], day[1], day[2], hours[1] + 1)) };
      return { since: dayStart(p.from), until: dayStart(p.to, 1) };
    }
  }
}

/** A day as M/D. */
function shortDay(value: string | undefined): string {
  const p = dayParts(value);
  return p ? `${p[1] + 1}/${p[2]}` : "";
}

/** The selection in words, for the period button. */
export function periodLabel(p: PeriodSel): string {
  if (p.kind !== "range") return t(`history.period.${p.kind}`);
  if (!p.from && !p.to) return t("history.period.all");
  const hours = hoursOf(p);
  if (hours) return `${shortDay(p.from)} ${t("history.period.span", { a: `${hours[0]}:00`, b: `${hours[1] + 1}:00` })}`;
  if (!p.to || p.from === p.to) return shortDay(p.from || p.to);
  if (!p.from) return t("history.period.until", { day: shortDay(p.to) });
  return t("history.period.span", { a: shortDay(p.from), b: shortDay(p.to) });
}
