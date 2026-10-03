import type { StatsDay, StatsNamed } from "./api";

export type Period = "today" | "week" | "month" | "all";

const DAY = 86400;

function midnight(d: Date, daysBack: number): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() - daysBack).getTime() / 1000;
}

/** Start of the period in seconds since the epoch, at a local midnight; 0 for all. */
export function periodStart(period: Period, now: Date): number {
  if (period === "all") return 0;
  return midnight(now, period === "today" ? 0 : period === "week" ? 6 : 29);
}

/** Local days since the epoch for a date. */
export function localDay(d: Date): number {
  return Math.floor((d.getTime() / 1000 - d.getTimezoneOffset() * 60) / DAY);
}

/** The local date of a day number from `localDay`. */
export function dayDate(day: number): Date {
  const utc = new Date(day * DAY * 1000);
  return new Date(utc.getUTCFullYear(), utc.getUTCMonth(), utc.getUTCDate());
}

/** Current and longest run of consecutive days in `days` (ascending). A run that ended yesterday is still current. */
export function streaks(days: number[], today: number): { current: number; longest: number } {
  let longest = 0, run = 0, prev: number | null = null;
  for (const d of days) {
    run = prev !== null && d === prev + 1 ? run + 1 : 1;
    longest = Math.max(longest, run);
    prev = d;
  }
  const current = prev !== null && prev >= today - 1 ? run : 0;
  return { current, longest };
}

/** Share of `part` in `whole` as a whole percentage; 0 when `whole` is 0. */
export function percent(part: number, whole: number): number {
  return whole > 0 ? Math.round((part / whole) * 100) : 0;
}

/** One figure per day from `from` to `to` inclusive (characters unless another field is named), 0 where a day is missing. */
export function fillDays(rows: StatsDay[], from: number, to: number, field: "chars" | "count" | "audio_ms" = "chars"): number[] {
  const byDay = new Map(rows.map((r) => [r.day, r[field] ?? 0]));
  const out: number[] = [];
  for (let d = from; d <= to; d++) out.push(byDay.get(d) ?? 0);
  return out;
}

/** Heatmap columns: 14 px squares with a 4 px gap. */
export const HEAT_CELL = 14;
export const HEAT_GAP = 4;
/** Fewest and most weeks the heatmap shows: ten in a one-column card, up to a year when the card is wide. */
export const HEAT_WEEKS_MIN = 10;
export const HEAT_WEEKS_MAX = 52;

/** How many weeks of squares fit in `width` pixels, within the limits; 0 or less (not yet measured) gives the minimum. */
export function heatWeeks(width: number): number {
  const fit = Math.floor((width + HEAT_GAP) / (HEAT_CELL + HEAT_GAP));
  return Math.max(HEAT_WEEKS_MIN, Math.min(HEAT_WEEKS_MAX, fit));
}

/** One label per heatmap column: the month's name on the column that holds its first day, "" elsewhere. */
export function monthLabels(from: number, weeks: number, name: (d: Date) => string): string[] {
  const out: string[] = [];
  for (let c = 0; c < weeks; c++) {
    let label = "";
    for (let i = 0; i < 7; i++) {
      const d = dayDate(from + c * 7 + i);
      if (d.getDate() === 1) {
        label = name(d);
        break;
      }
    }
    out.push(label);
  }
  return out;
}

/** The change from `before` to `now` as a whole percentage; null when there is nothing to compare with. */
export function change(now: number, before: number): number | null {
  if (before <= 0) return null;
  return Math.round(((now - before) / before) * 100);
}

/** Heat level 0 to 4 of a day's characters against the largest day; any use is at least 1. */
export function heatLevel(chars: number, max: number): 0 | 1 | 2 | 3 | 4 {
  if (chars <= 0 || max <= 0) return 0;
  return Math.min(4, Math.max(1, Math.ceil((chars / max) * 4))) as 1 | 2 | 3 | 4;
}

/** The first `max` rows and what the rest add up to (null when nothing is left). */
export function topWithOthers(rows: StatsNamed[], max: number): { top: StatsNamed[]; others: { count: number; names: number } | null } {
  const rest = rows.slice(max);
  return {
    top: rows.slice(0, max),
    others: rest.length ? { count: rest.reduce((n, r) => n + r.count, 0), names: rest.length } : null,
  };
}

/** Characters per minute, or null when there is no audio to measure against. */
export function speed(chars: number, audioMs: number): number | null {
  return audioMs > 0 ? Math.round((chars * 60000) / audioMs) : null;
}

export function durationParts(ms: number): { hours: number; minutes: number; seconds: number } {
  const total = Math.floor(ms / 1000);
  return { hours: Math.floor(total / 3600), minutes: Math.floor((total % 3600) / 60), seconds: total % 60 };
}

/** Sums of `values` over consecutive groups of `size`, the last group possibly shorter. */
export function groupSums(values: number[], size: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < values.length; i += size) out.push(values.slice(i, i + size).reduce((a, b) => a + b, 0));
  return out;
}
