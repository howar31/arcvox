import { describe, expect, it } from "vitest";
import type { Stats } from "./api";
import { cardFigures, cardHeight, cardSvg, chartData, DEFAULT_OPTIONS, fileName, HEAT_DAYS, heatColumns, heatGrid, normaliseOptions, textWidth, type ShareOptions, type ShareStrings } from "./shareCard";
import { localDay } from "./statsView";

const now = new Date(2026, 9, 23, 15, 0, 0);
const today = localDay(now);

const strings: ShareStrings = {
  period: { week: "最近 7 天", month: "最近 30 天", year: "今年", all: "累計" },
  caption: { chars: "用說的寫出的字數", saved: "估計省下的打字時間", count: "聽寫次數", days: "使用天數", streak: "連續使用", cpm: "說話速度", peak: "單則最高字數", audio: "聽寫時間" },
  unit: { chars: "字", count: "次", days: "天", cpm: "字/分", hours: "小時", minutes: "分" },
  unavailable: { saved: "還沒有可估計的資料", cpm: "還沒有三秒以上的聽寫" },
  chart: { days: "最近 {n} 天，每格一天", daily: "每日輸出字數", weekly: "每週輸出字數" },
};

function stats(over: Partial<Stats> = {}): Stats {
  const days = [today - 2, today - 1, today].map((day, i) => ({ day, count: 3 + i, chars: 400 * (i + 1), audio_ms: 60_000 * (i + 1) }));
  return {
    totals: {
      count: 12, chars: 128_460, audio_ms: 5_400_000, timed_count: 10, timed_chars: 120_000, speed_chars: 5_460, speed_audio_ms: 30 * 60_000,
      asr_ms: 0, llm_ms: 0, llm_count: 0, changed: 0, degraded: 0, unrewritten: 0, dict_fixes: 0, snippets: 0,
      han: 0, latin: 0, kana: 0, max_chars: 980, max_audio_ms: 0, max_total_ms: 0, max_cpm: 0,
    },
    saved_ms: 6.4 * 3_600_000,
    days,
    hours: Array(24).fill(0),
    apps: [], profiles: [],
    active_days: [...Array.from({ length: 20 }, (_, i) => ({ day: today - 40 + i, count: 1, chars: 100, audio_ms: 1000 })), ...days],
    ...over,
  };
}

describe("the figures table", () => {
  it("formats every figure with its unit and caption", () => {
    const f = cardFigures(stats(), today, "en-US", strings);
    expect(f.chars).toEqual({ key: "chars", value: "128,460", unit: "字", caption: "用說的寫出的字數", available: true });
    expect([f.saved.value, f.saved.unit]).toEqual(["6.4", "小時"]);
    expect([f.count.value, f.days.value, f.streak.value]).toEqual(["12", "3", "3"]);
    expect([f.cpm.value, f.cpm.unit]).toEqual(["182", "字/分"]);
    expect(f.peak.value).toBe("980");
    expect([f.audio.value, f.audio.unit]).toEqual(["1.5", "小時"]);
  });

  it("gives minutes under an hour and marks figures that cannot be computed", () => {
    const f = cardFigures(stats({ saved_ms: 25 * 60_000 }), today, "en-US", strings);
    expect([f.saved.value, f.saved.unit]).toEqual(["25", "分"]);
    const none = cardFigures(stats({ saved_ms: null, totals: { ...stats().totals, speed_audio_ms: 0 } }), today, "en-US", strings);
    expect(none.saved).toMatchObject({ available: false, caption: "還沒有可估計的資料" });
    expect(none.cpm).toMatchObject({ available: false, caption: "還沒有三秒以上的聽寫" });
  });

  it("numbers use the locale", () => {
    expect(cardFigures(stats(), today, "zh-TW", strings).chars.value).toBe("128,460");
    expect(cardFigures(stats(), today, "de-DE", strings).chars.value).toBe("128.460");
  });

  it("empty stats give zeros", () => {
    const empty = stats({ totals: { ...stats().totals, count: 0, chars: 0, audio_ms: 0, speed_audio_ms: 0, max_chars: 0 }, saved_ms: null, days: [], active_days: [] });
    const f = cardFigures(empty, today, "en-US", strings);
    expect([f.chars.value, f.count.value, f.days.value, f.streak.value, f.peak.value]).toEqual(["0", "0", "0", "0", "0"]);
    expect([f.audio.value, f.audio.unit]).toEqual(["0", "分"]);
  });
});

describe("the options", () => {
  const figures = cardFigures(stats(), today, "en-US", strings);

  it("start from the defaults and keep known values", () => {
    expect(normaliseOptions(null, figures)).toEqual(DEFAULT_OPTIONS);
    expect(normaliseOptions({ period: "year", style: "brand", size: "story", chart: "none", main: "days", extra: ["count"] }, figures))
      .toEqual({ period: "year", style: "brand", size: "story", chart: "none", main: "days", extra: ["count"] });
    expect(normaliseOptions({ period: "x" as never, main: "streak" as never }, figures)).toMatchObject({ period: "month", main: "chars" });
  });

  it("keeps at most three secondary figures, none equal to the main one", () => {
    const o = normaliseOptions({ main: "count", extra: ["count", "saved", "streak", "cpm", "peak"] }, figures);
    expect(o.extra).toEqual(["saved", "streak", "cpm"]);
    expect(normaliseOptions({ extra: ["saved", "saved", "audio"] }, figures).extra).toEqual(["saved", "audio"]);
  });

  it("unavailable figures are dropped", () => {
    const none = cardFigures(stats({ saved_ms: null }), today, "en-US", strings);
    expect(normaliseOptions({ extra: ["saved", "streak"] }, none).extra).toEqual(["streak"]);
  });
});

describe("the chart", () => {
  it("the activity grid has the last year of days, today last", () => {
    const c = chartData(stats(), DEFAULT_OPTIONS, today, now) as { kind: "heat"; chars: number[] };
    expect(c.kind).toBe("heat");
    expect(c.chars).toHaveLength(HEAT_DAYS);
    expect(c.chars.at(-1)).toBe(1200);
  });

  it("bars are daily for a month and weekly for a year", () => {
    const month = chartData(stats(), { ...DEFAULT_OPTIONS, chart: "daily" }, today, now) as { values: number[]; weekly: boolean };
    expect(month.weekly).toBe(false);
    expect(month.values).toHaveLength(30);
    const year = chartData(stats(), { ...DEFAULT_OPTIONS, chart: "daily", period: "year" }, today, now) as { values: number[]; weekly: boolean };
    expect(year.weekly).toBe(true);
    expect(year.values.length).toBeGreaterThan(40);
    expect(chartData(stats(), { ...DEFAULT_OPTIONS, chart: "none" }, today, now)).toBeNull();
  });
});

describe("the SVG", () => {
  const svg = (o: Partial<ShareOptions> = {}) => cardSvg(stats(), { ...DEFAULT_OPTIONS, ...o }, strings, today, "en-US", now);

  it("carries the figures, the brand and the address, and nothing else", () => {
    const s = svg();
    expect(s).toContain('viewBox="0 0 1080 1080"');
    expect(s).toContain(">128,460<");
    expect(s).toContain(">Arcvox<");
    expect(s).toContain("arcvox.howar31.com");
    expect(s).toContain("最近 30 天");
    expect(s).toMatch(/最近 \d+ 天，每格一天/);
    expect(s).toContain("估計省下的打字時間");
    expect(s).not.toContain("foreignObject");
    // The fixture has records over 41 days: the grid holds at least that many, in cells larger than the
    // finest, and every cell of the grid is drawn.
    const cells = (s.match(/<rect /g) ?? []).length - 1;
    expect(cells).toBeGreaterThanOrEqual(41);
    expect(s).toContain(`最近 ${cells} 天，每格一天`);
  });

  it("every size places the foot inside the height", () => {
    for (const size of ["square", "portrait", "story"] as const) {
      const s = svg({ size });
      const h = cardHeight(size);
      expect(s).toContain(`viewBox="0 0 1080 ${h}"`);
      const ys = [...s.matchAll(/ y="([\d.]+)"/g)].map((m) => Number(m[1]));
      expect(Math.max(...ys)).toBeLessThanOrEqual(h - 72);
    }
  });

  it("a long number shrinks the hero to the width", () => {
    const big = stats({ totals: { ...stats().totals, chars: 12_345_678 } });
    const s = cardSvg(big, { ...DEFAULT_OPTIONS, size: "story" }, strings, today, "en-US", now);
    const size = Number(/<tspan font-size="(\d+)" font-weight="800"/.exec(s)?.[1]);
    expect(size).toBeLessThan(300);
    expect(size).toBeGreaterThanOrEqual(120);
    expect(s).toContain(">12,345,678<");
  });

  it("styles and charts change the drawing, not the figures", () => {
    expect(svg({ style: "brand" })).toContain('fill="url(#brand)"');
    expect(svg({ style: "light" })).toContain('fill="#FFFFFF"');
    const bars = svg({ chart: "daily" });
    expect(bars).toContain("每日輸出字數");
    expect(bars).toContain('fill="url(#bar)"');
    expect(svg({ chart: "none" })).not.toContain("每格一天");
    expect(svg({ extra: [] })).not.toContain("連續使用");
  });

  it("escapes text", () => {
    expect(cardSvg(stats(), DEFAULT_OPTIONS, { ...strings, period: { ...strings.period, month: "a<b" } }, today, "en-US", now)).toContain("a&lt;b");
  });
});

it("the grid takes the largest cells that hold the days with a record", () => {
  expect(heatColumns("square")).toBe(26);
  expect(heatColumns("story")).toBe(heatColumns("square"));
  const year = heatGrid(936, 300, 30, 365);
  expect(year.cols * year.rows).toBeLessThan(365);
  expect(year.cell).toBe(30);
  const month = heatGrid(936, 300, 30, 28);
  expect(month.cols * month.rows).toBeGreaterThanOrEqual(28);
  expect(month.cell).toBe(40);
  // A tall room with the finest cells never holds more than a year.
  const tall = heatGrid(936, 1200, 30, 365);
  expect(tall.cols * tall.rows).toBeLessThanOrEqual(HEAT_DAYS);
  // A taller card draws more days in cells of the same size.
  const cellOf = (size: "square" | "story") => Number(/<rect [^>]*width="([\d.]+)" height="\1" rx/.exec(cardSvg(stats(), { ...DEFAULT_OPTIONS, size }, strings, today, "en-US", now))?.[1]);
  expect(cellOf("story")).toBe(cellOf("square"));
  const cellsOf = (size: "square" | "story") => (cardSvg(stats(), { ...DEFAULT_OPTIONS, size }, strings, today, "en-US", now).match(/<rect /g) ?? []).length - 1;
  expect(cellsOf("story")).toBeGreaterThan(cellsOf("square"));
  const long = stats({ active_days: Array.from({ length: 400 }, (_, i) => ({ day: today - 399 + i, count: 1, chars: 10, audio_ms: 10 })) });
  const cells = (cardSvg(long, DEFAULT_OPTIONS, strings, today, "en-US", now).match(/<rect /g) ?? []).length - 1;
  expect(cells).toBeLessThanOrEqual(HEAT_DAYS);
  expect(cells % heatColumns("square")).toBe(0);
});

it("the grid runs down each column with today at the bottom right, like the Statistics page", () => {
  const s = cardSvg(stats(), DEFAULT_OPTIONS, strings, today, "en-US", now);
  const cells = [...s.matchAll(/<rect x="([\d.]+)" y="([\d.]+)" width="([\d.]+)" height="\3" rx/g)].map((m) => ({ x: Number(m[1]), y: Number(m[2]) }));
  const last = cells.at(-1)!, before = cells.at(-2)!;
  expect(Math.max(...cells.map((c) => c.x))).toBe(last.x);
  expect(Math.max(...cells.map((c) => c.y))).toBe(last.y);
  expect(before.x).toBe(last.x);
  expect(before.y).toBeLessThan(last.y);
});

it("measures text roughly and names the file by the date", () => {
  expect(textWidth("12", 100)).toBe(120);
  expect(textWidth("字", 100)).toBe(100);
  expect(fileName("統計", now)).toBe("Arcvox 統計 2026-10-23.png");
});
