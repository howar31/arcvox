import type { Stats } from "./api";
import { fillDays, groupSums, heatLevel, localDay, periodStart, speed, streaks } from "./statsView";

// The share card: the user's figures as one image for a social post. A
// pure module: the figures table, the options and the SVG; the dialog
// renders the SVG and the platform writes the PNG.

export type SharePeriod = "week" | "month" | "year" | "all";
export type ShareStyle = "dark" | "light" | "brand";
export type ShareSize = "square" | "portrait" | "story";
export type ShareChart = "days" | "daily" | "none";
export type FigureKey = "chars" | "saved" | "count" | "days" | "streak" | "cpm" | "peak" | "audio";

export interface ShareOptions { period: SharePeriod; main: FigureKey; extra: FigureKey[]; chart: ShareChart; style: ShareStyle; size: ShareSize }

export const PERIODS: readonly SharePeriod[] = ["week", "month", "year", "all"];
export const STYLES: readonly ShareStyle[] = ["dark", "light", "brand"];
export const SIZES: readonly ShareSize[] = ["square", "portrait", "story"];
export const CHARTS: readonly ShareChart[] = ["days", "daily", "none"];
/** The figures the main place can show. */
export const MAIN_KEYS: readonly FigureKey[] = ["chars", "saved", "count", "days"];
/** The figures the row under it can show. */
export const EXTRA_KEYS: readonly FigureKey[] = ["saved", "streak", "cpm", "count", "peak", "audio"];
export const MAX_EXTRA = 3;
export const DEFAULT_OPTIONS: ShareOptions = { period: "month", main: "chars", extra: ["saved", "streak", "cpm"], chart: "days", style: "dark", size: "square" };

/** The card's width in units; the height is the size's. */
export const CARD_WIDTH = 1080;
const HEIGHTS: Record<ShareSize, number> = { square: 1080, portrait: 1350, story: 1920 };
export const cardHeight = (size: ShareSize): number => HEIGHTS[size];
/** Days the activity grid can reach back; the card shows as many of the latest as its grid holds. */
export const HEAT_DAYS = 365;
const HEAT_GAP = 6;
/** The cells of the activity grid, the same on every card size: the largest that hold the days with a record. */
const HEAT_CELL_MAX = 40;
const HEAT_CELL_MIN = 30;

/** The largest cells, at least `min`, whose grid in `width` x `height` holds `days`; the finest grid when none does. Every cell of the grid is drawn, so it never holds more than a year. */
export function heatGrid(width: number, height: number, min: number, days: number): { cell: number; cols: number; rows: number } {
  const grid = (cell: number) => {
    const cols = Math.max(1, Math.floor((width + HEAT_GAP) / (cell + HEAT_GAP)));
    const rows = Math.max(1, Math.min(Math.floor((height + HEAT_GAP) / (cell + HEAT_GAP)), Math.floor(HEAT_DAYS / cols)));
    return { cell, cols, rows };
  };
  for (let cell = HEAT_CELL_MAX; cell > min; cell -= 1) {
    const g = grid(cell);
    if (g.cols * g.rows >= days) return g;
  }
  return grid(min);
}
export const SITE = "arcvox.howar31.com";

/** One figure as the card shows it. `available` is false when the value cannot be computed (the caption says why). */
export interface Figure { key: FigureKey; value: string; unit: string; caption: string; available: boolean }

/** The wording the card and the figures need, in the interface language. */
export interface ShareStrings {
  period: Record<SharePeriod, string>;
  caption: Record<FigureKey, string>;
  unit: { chars: string; count: string; days: string; cpm: string; hours: string; minutes: string };
  unavailable: { saved: string; cpm: string };
  chart: { days: string; daily: string; weekly: string };
}

const fmt = (n: number, locale: string, digits = 0) => n.toLocaleString(locale, { maximumFractionDigits: digits, minimumFractionDigits: digits });

/** Hours with one decimal from an hour up, else whole minutes. */
function duration(ms: number, locale: string, s: ShareStrings): { value: string; unit: string } {
  const minutes = ms / 60000;
  return minutes >= 60 ? { value: fmt(minutes / 60, locale, 1), unit: s.unit.hours } : { value: fmt(Math.round(minutes), locale), unit: s.unit.minutes };
}

export function cardFigures(stats: Stats, today: number, locale: string, s: ShareStrings): Record<FigureKey, Figure> {
  const t = stats.totals;
  const saved = stats.saved_ms === null ? null : duration(stats.saved_ms, locale, s);
  const cpm = speed(t.speed_chars, t.speed_audio_ms);
  const audio = duration(t.audio_ms, locale, s);
  const current = streaks(stats.active_days.map((d) => d.day), today).current;
  const fig = (key: FigureKey, value: string, unit: string, caption = s.caption[key]): Figure => ({ key, value, unit, caption, available: true });
  return {
    chars: fig("chars", fmt(t.chars, locale), s.unit.chars),
    saved: saved ? fig("saved", saved.value, saved.unit) : { key: "saved", value: "", unit: "", caption: s.unavailable.saved, available: false },
    count: fig("count", fmt(t.count, locale), s.unit.count),
    days: fig("days", fmt(stats.days.length, locale), s.unit.days),
    streak: fig("streak", fmt(current, locale), s.unit.days),
    cpm: cpm === null ? { key: "cpm", value: "", unit: "", caption: s.unavailable.cpm, available: false } : fig("cpm", fmt(cpm, locale), s.unit.cpm),
    peak: fig("peak", fmt(t.max_chars, locale), s.unit.chars),
    audio: fig("audio", audio.value, audio.unit),
  };
}

/** Options as stored or picked, brought within the rules: a main figure from its list, at most three extra ones that exist, are available and differ from the main one. */
export function normaliseOptions(o: Partial<ShareOptions> | null | undefined, figures: Record<FigureKey, Figure>): ShareOptions {
  const d = DEFAULT_OPTIONS;
  const pick = <T extends string>(v: unknown, list: readonly T[], fallback: T): T => (list.includes(v as T) ? (v as T) : fallback);
  const main = pick(o?.main, MAIN_KEYS, d.main);
  const wanted = Array.isArray(o?.extra) ? o!.extra : d.extra;
  const extra = wanted.filter((k, i) => EXTRA_KEYS.includes(k) && k !== main && figures[k]?.available && wanted.indexOf(k) === i).slice(0, MAX_EXTRA);
  return { period: pick(o?.period, PERIODS, d.period), main, extra, chart: pick(o?.chart, CHARTS, d.chart), style: pick(o?.style, STYLES, d.style), size: pick(o?.size, SIZES, d.size) };
}

interface Palette { bg: string; ink: string; soft: string; hero: [string, string] | null; heat: string[]; bar: [string, string]; glows: boolean; markWhite: boolean }
const PALETTES: Record<ShareStyle, Palette> = {
  dark: { bg: "#0F1220", ink: "#F4F5FA", soft: "#F4F5FA", hero: ["#FFA8D0", "#BFE3FF"], heat: ["rgba(255,255,255,0.08)", "rgba(143,162,255,0.35)", "rgba(143,162,255,0.6)", "#8FA2FF", "#FFA8D0"], bar: ["#FFA8D0", "#8FA2FF"], glows: true, markWhite: false },
  light: { bg: "#FFFFFF", ink: "#15171C", soft: "#15171C", hero: ["#2747D6", "#FF7AB8"], heat: ["#EEF0F4", "#C9D3FF", "#8FA2FF", "#2747D6", "#FF7AB8"], bar: ["#2747D6", "#2747D6"], glows: false, markWhite: false },
  // On the gradient the grid's levels run from the brand's ice blue to white (owner's pick D, 2026-10-24); an empty day is a faint white.
  brand: { bg: "", ink: "#FFFFFF", soft: "#FFFFFF", hero: null, heat: ["rgba(255,255,255,0.14)", "rgba(127,196,255,0.5)", "rgba(170,215,255,0.75)", "rgba(215,235,255,0.92)", "#FFFFFF"], bar: ["#FFFFFF", "rgba(255,255,255,0.75)"], glows: false, markWhite: true },
};

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
const FONT = `-apple-system, "PingFang TC", "Helvetica Neue", "Segoe UI", sans-serif`;
const PAD = 72;

/** The brand mark as in `Mark.svelte`, drawn at `size` with its top left at (x, y). */
function mark(x: number, y: number, size: number, fill: string): string {
  const k = size / 75;
  return `<g transform="translate(${x} ${y}) scale(${k}) translate(-12.5 -12.5)"><path d="M80.96 48.38A31 31 0 1 1 51.62 19.04" fill="none" stroke="${fill}" stroke-width="13" stroke-linecap="round"/><circle cx="71.92" cy="28.08" r="7.8" fill="${fill}"/></g>`;
}

/** Rough text width for laying the unit after a number: digits and ASCII at 0.6 em, CJK at 1 em. */
export function textWidth(text: string, size: number): number {
  let w = 0;
  for (const c of text) w += c.charCodeAt(0) > 0x2e7f ? 1 : c === "," || c === "." ? 0.25 : 0.6;
  return w * size;
}

/** The chart's data: heat levels for 26 weeks, or bars for the period (per week when it is long). */
export function chartData(stats: Stats, options: ShareOptions, today: number, now: Date): { kind: "heat"; chars: number[]; days: number } | { kind: "bars"; values: number[]; weekly: boolean } | null {
  if (options.chart === "none") return null;
  if (options.chart === "days") {
    // The characters of each of the last 365 days, oldest first, and how many of them reach back to the
    // first day with a record; the grid is sized to hold those and every cell of it is drawn.
    const first = stats.active_days[0]?.day;
    const days = Math.min(HEAT_DAYS, Math.max(1, first === undefined ? 1 : today - first + 1));
    return { kind: "heat", chars: fillDays(stats.active_days, today - HEAT_DAYS + 1, today), days };
  }
  const first = options.period === "all" ? (stats.days[0]?.day ?? today) : localDay(new Date(periodStart(options.period, now) * 1000));
  const values = fillDays(stats.days, Math.min(first, today), today);
  const weekly = values.length > 31;
  return { kind: "bars", values: weekly ? groupSums(values, 7) : values, weekly };
}

/** Type sizes and gaps per card size; a tall card stacks the secondary figures. */
interface Layout { hero: number; heroCaption: number; sub: number; subCaption: number; column: boolean; rowGap: number; gap: [number, number, number, number]; heatCell: number }
const LAYOUTS: Record<ShareSize, Layout> = {
  square: { hero: 200, heroCaption: 40, sub: 76, subCaption: 28, column: false, rowGap: 0, gap: [90, 52, 52, 40], heatCell: HEAT_CELL_MIN },
  portrait: { hero: 240, heroCaption: 46, sub: 92, subCaption: 32, column: false, rowGap: 0, gap: [100, 60, 60, 48], heatCell: HEAT_CELL_MIN },
  story: { hero: 300, heroCaption: 56, sub: 104, subCaption: 36, column: false, rowGap: 0, gap: [110, 64, 64, 52], heatCell: HEAT_CELL_MIN },
};

/** The columns of the activity grid at this size: as many cells of the size's measure as fit the width. */
export function heatColumns(size: ShareSize): number {
  return Math.floor((CARD_WIDTH - 2 * PAD + HEAT_GAP) / (LAYOUTS[size].heatCell + HEAT_GAP));
}

/** The width of the hero number at `size`: a heavy face, wider than the estimate for text. */
function heroWidth(value: string, size: number): number {
  let w = 0;
  for (const c of value) w += c === "," || c === "." ? 0.3 : 0.66;
  return w * size - size * 0.035 * (value.length - 1);
}

/** The whole card as an SVG document. */
export function cardSvg(stats: Stats, options: ShareOptions, s: ShareStrings, today: number, locale: string, now = new Date()): string {
  const W = CARD_WIDTH, H = cardHeight(options.size);
  const p = PALETTES[options.style];
  const figures = cardFigures(stats, today, locale, s);
  const main = figures[options.main];
  const extra = options.extra.map((k) => figures[k]).filter((f) => f.available);
  const chart = chartData(stats, options, today, now);
  const parts: string[] = [];
  parts.push(`<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" font-family='${FONT}'>`);
  parts.push(`<defs>
    <linearGradient id="brand" gradientUnits="userSpaceOnUse" x1="0" y1="${H}" x2="${W}" y2="0"><stop offset="0" stop-color="#FF7AB8"/><stop offset="0.58" stop-color="#2747D6"/><stop offset="1" stop-color="#7FC4FF"/></linearGradient>
    <linearGradient id="markg" gradientUnits="objectBoundingBox" x1="1" y1="1" x2="0" y2="0"><stop offset="0" stop-color="#FF7AB8"/><stop offset="0.52" stop-color="#2747D6"/><stop offset="1" stop-color="#7FC4FF"/></linearGradient>
    ${p.hero ? `<linearGradient id="hero" gradientUnits="objectBoundingBox" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="${p.hero[0]}"/><stop offset="1" stop-color="${p.hero[1]}"/></linearGradient>` : ""}
    <linearGradient id="bar" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${p.bar[0]}"/><stop offset="1" stop-color="${p.bar[1]}"/></linearGradient>
    <radialGradient id="glow1"><stop offset="0" stop-color="#FF7AB8" stop-opacity="0.45"/><stop offset="0.55" stop-color="#2747D6" stop-opacity="0.25"/><stop offset="1" stop-color="#2747D6" stop-opacity="0"/></radialGradient>
    <radialGradient id="glow2"><stop offset="0" stop-color="#7FC4FF" stop-opacity="0.35"/><stop offset="1" stop-color="#7FC4FF" stop-opacity="0"/></radialGradient>
    <filter id="shadow" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="8" stdDeviation="10" flood-color="#000000" flood-opacity="${p.bg === "#FFFFFF" ? 0.18 : 0.4}"/></filter>
  </defs>`);
  parts.push(`<rect width="${W}" height="${H}" fill="${p.bg || "url(#brand)"}"/>`);
  if (p.glows) parts.push(`<circle cx="${W - 130}" cy="30" r="550" fill="url(#glow1)"/><circle cx="70" cy="${H + 30}" r="450" fill="url(#glow2)"/>`);
  const ink = p.ink;
  const L0 = LAYOUTS[options.size];
  // The hero shrinks when the number and its unit would run past the width.
  const heroNeed = heroWidth(main.value, L0.hero) + L0.hero * 0.1 + textWidth(main.unit, L0.hero * 0.4);
  const heroFit = Math.min(1, (W - 2 * PAD - 24) / heroNeed);
  const L = { ...L0, hero: Math.floor(L0.hero * heroFit) };
  // Natural heights of the blocks, top to bottom; the room left goes to the chart first, then the gaps.
  const heroH = L.hero + L.hero * 0.22 + L.heroCaption + 16;
  const subsH = extra.length === 0 ? 0 : L.column ? extra.length * (L.sub + 12 + L.subCaption + L.rowGap) - L.rowGap : L.sub + 12 + L.subCaption + 4;
  const heatCols = heatColumns(options.size);
  const heatCell = (W - 2 * PAD - HEAT_GAP * (heatCols - 1)) / heatCols;
  // The activity grid wants a week of rows at least and fills whatever room is left; the bars grow to a fixed height.
  const chartNatural = chart ? (chart.kind === "heat" ? heatCell * 7 + HEAT_GAP * 6 : 240) : 0;
  const chartMax = chart ? (chart.kind === "heat" ? H : 520) : 0;
  const footH = 28;
  const gaps = [L.gap[0], L.gap[1], subsH ? L.gap[2] : 0, chart ? L.gap[3] : 0];
  let room = H - 2 * PAD - 60 - heroH - subsH - chartNatural - footH - gaps.reduce((a, b) => a + b, 0);
  let chartH = chartNatural;
  if (room < 0) {
    // Too tall for the card: the gaps give way first, then the chart.
    const total = gaps.reduce((a, b) => a + b, 0);
    const keep = Math.max(0.35, 1 + room / total);
    for (let i = 0; i < gaps.length; i++) gaps[i] *= keep;
    room += total * (1 - keep);
    if (room < 0 && chart) {
      chartH = Math.max(120, chartH + room);
      room = 0;
    }
  }
  if (chart && room > 0) {
    // The chart takes the room left, up to its cap; the gaps share the rest.
    const grow = Math.min(room, chartMax - chartNatural);
    chartH += grow;
    room -= grow;
  }
  if (room > 0) {
    const weights = [2, 2, subsH ? 2 : 0, chart ? 2 : 0];
    const total = weights.reduce((a, b) => a + b, 0);
    for (let i = 0; i < gaps.length; i++) gaps[i] += (room * weights[i]) / total;
  }
  // Brand row.
  let y = PAD;
  parts.push(mark(PAD, y, 60, p.markWhite ? "#FFFFFF" : "url(#markg)"));
  parts.push(`<text x="${PAD + 78}" y="${y + 44}" font-size="40" font-weight="700" fill="${ink}">Arcvox</text>`);
  parts.push(`<text x="${W - PAD}" y="${y + 40}" font-size="30" fill="${ink}" fill-opacity="0.72" text-anchor="end">${esc(s.period[options.period])}</text>`);
  // Hero: the unit follows the number in the same text element, so the renderer places it after the real width.
  y += 60 + gaps[0] + L.hero;
  const heroFill = p.hero ? "url(#hero)" : ink;
  parts.push(`<text x="${PAD - 8}" y="${y.toFixed(1)}" fill="${heroFill}"><tspan font-size="${L.hero}" font-weight="800" letter-spacing="${-L.hero * 0.035}">${esc(main.value)}</tspan><tspan font-size="${L.hero * 0.4}" font-weight="700" dx="${L.hero * 0.1}">${esc(main.unit)}</tspan></text>`);
  y += L.hero * 0.22 + L.heroCaption;
  parts.push(`<text x="${PAD}" y="${y.toFixed(1)}" font-size="${L.heroCaption}" fill="${ink}" fill-opacity="0.78">${esc(main.caption)}</text>`);
  y += 16;
  // Secondary figures: one row, or one under the other on a tall card.
  if (extra.length) {
    y += gaps[1];
    let x = PAD;
    for (const f of extra) {
      const base = y + L.sub;
      parts.push(`<text x="${x}" y="${base.toFixed(1)}" fill="${ink}"><tspan font-size="${L.sub}" font-weight="700" letter-spacing="${-L.sub * 0.026}">${esc(f.value)}</tspan><tspan font-size="${L.sub * 0.47}" font-weight="600" dx="${L.sub * 0.13}" fill-opacity="0.85">${esc(f.unit)}</tspan></text>`);
      parts.push(`<text x="${x}" y="${(base + 12 + L.subCaption).toFixed(1)}" font-size="${L.subCaption}" fill="${ink}" fill-opacity="0.7">${esc(f.caption)}</text>`);
      if (L.column) {
        y += L.sub + 12 + L.subCaption + L.rowGap;
      } else {
        const vw = textWidth(f.value, L.sub) + L.sub * 0.13 + textWidth(f.unit, L.sub * 0.47);
        x += Math.max(vw, textWidth(f.caption, L.subCaption)) + 56;
      }
    }
    y += L.column ? -L.rowGap : L.sub + 12 + L.subCaption + 4;
  }
  // Chart.
  const footY = H - PAD;
  let heatShown = 0;
  if (chart) {
    y += gaps[2];
    const top = y;
    if (chart.kind === "heat") {
      // A grid of the latest days, one cell each, every cell drawn, in the order of the Statistics page's
      // heatmap: down each column, columns from the oldest at the left to today at the bottom right. The
      // cells are the size's measure at least, and larger when the days with a record fill a coarser grid.
      const { cell, cols, rows } = heatGrid(W - 2 * PAD, chartH, heatCell, chart.days);
      const shown = Math.min(chart.chars.length, rows * cols);
      const chars = chart.chars.slice(chart.chars.length - shown);
      const max = Math.max(0, ...chars);
      const left = PAD + (W - 2 * PAD - (cols * cell + HEAT_GAP * (cols - 1))) / 2;
      const topY = top + (chartH - (rows * cell + HEAT_GAP * (rows - 1))) / 2;
      const first = rows * cols - shown;
      chars.forEach((c, i) => {
        const at = first + i;
        const cx = left + Math.floor(at / rows) * (cell + HEAT_GAP);
        const cy = topY + (at % rows) * (cell + HEAT_GAP);
        parts.push(`<rect x="${cx.toFixed(1)}" y="${cy.toFixed(1)}" width="${cell.toFixed(1)}" height="${cell.toFixed(1)}" rx="${Math.min(14, cell / 4).toFixed(1)}" fill="${p.heat[heatLevel(c, max)]}"/>`);
      });
      heatShown = shown;
    } else {
      const n = chart.values.length;
      const max = Math.max(1, ...chart.values);
      const gap = n > 40 ? 6 : 14;
      const bw = (W - 2 * PAD - gap * (n - 1)) / n;
      const bottom = top + chartH;
      parts.push(`<g filter="url(#shadow)">`);
      chart.values.forEach((v, i) => {
        const bh = Math.max(v > 0 ? 6 : 0, (v / max) * chartH);
        parts.push(`<rect x="${(PAD + i * (bw + gap)).toFixed(1)}" y="${(bottom - bh).toFixed(1)}" width="${bw.toFixed(1)}" height="${bh.toFixed(1)}" rx="${Math.min(8, bw / 3).toFixed(1)}" fill="url(#bar)"/>`);
      });
      parts.push(`</g>`);
    }
    const caption = chart.kind === "heat" ? s.chart.days.replace("{n}", String(heatShown)) : chart.weekly ? s.chart.weekly : s.chart.daily;
    parts.push(`<text x="${PAD}" y="${footY}" font-size="28" fill="${ink}" fill-opacity="0.6">${esc(caption)}</text>`);
  }
  parts.push(`<text x="${W - PAD}" y="${footY}" font-size="28" fill="${ink}" fill-opacity="0.6" text-anchor="end">${SITE}</text>`);
  parts.push("</svg>");
  return parts.join("\n");
}

/** The SVG as a data URL for an `<img>`. */
export const svgDataUrl = (svg: string): string => `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;

/** The file name of a saved card: the app's name, the word for statistics and the local date. */
export function fileName(word: string, now: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `Arcvox ${word} ${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}.png`;
}
