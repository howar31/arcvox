import { describe, expect, it } from "vitest";
import { change, durationParts, fillDays, heatLevel, heatWeeks, localDay, monthLabels, percent, periodStart, speed, streaks, topWithOthers } from "./statsView";

describe("statsView", () => {
  const now = new Date(2026, 9, 1, 14, 30);

  it("starts periods at local midnight", () => {
    expect(periodStart("today", now)).toBe(new Date(2026, 9, 1).getTime() / 1000);
    expect(periodStart("week", now)).toBe(new Date(2026, 8, 25).getTime() / 1000);
    expect(periodStart("month", now)).toBe(new Date(2026, 8, 2).getTime() / 1000);
    expect(periodStart("all", now)).toBe(0);
  });

  it("counts streaks, and a run that ended before yesterday is not current", () => {
    const t = localDay(now);
    expect(streaks([], t)).toEqual({ current: 0, longest: 0 });
    expect(streaks([t - 5, t - 4, t - 3, t - 1, t], t)).toEqual({ current: 2, longest: 3 });
    expect(streaks([t - 2, t - 1], t)).toEqual({ current: 2, longest: 2 });
    expect(streaks([t - 3, t - 2], t)).toEqual({ current: 0, longest: 2 });
  });

  it("never divides by zero", () => {
    expect(percent(0, 0)).toBe(0);
    expect(percent(1, 3)).toBe(33);
    expect(speed(100, 0)).toBeNull();
    expect(speed(146, 60000)).toBe(146);
    expect(heatLevel(0, 0)).toBe(0);
    expect(heatLevel(10, 10)).toBe(4);
    expect(heatLevel(1, 100)).toBe(1);
  });

  it("fills missing days and folds the tail of a list", () => {
    expect(fillDays([{ day: 3, count: 1, chars: 5, audio_ms: 900 }], 2, 4)).toEqual([0, 5, 0]);
    expect(fillDays([{ day: 3, count: 1, chars: 5, audio_ms: 900 }], 2, 4, "audio_ms")).toEqual([0, 900, 0]);
    expect(fillDays([], 4, 2)).toEqual([]);
    const rows = ["a", "b", "c"].map((name, i) => ({ name, count: 3 - i, chars: 0 }));
    expect(topWithOthers(rows, 2)).toEqual({ top: rows.slice(0, 2), others: { count: 1, names: 1 } });
    expect(topWithOthers(rows, 5).others).toBeNull();
  });

  it("splits a duration into hours, minutes and seconds", () => {
    expect(durationParts(0)).toEqual({ hours: 0, minutes: 0, seconds: 0 });
    expect(durationParts(2 * 3600_000 + 6 * 60_000 + 7_400)).toEqual({ hours: 2, minutes: 6, seconds: 7 });
  });

  it("gives the change as a whole percentage, or nothing without a base", () => {
    expect(change(112, 100)).toBe(12);
    expect(change(90, 100)).toBe(-10);
    expect(change(5, 0)).toBeNull();
  });

  it("shows as many heatmap weeks as fit, between 10 and 52", () => {
    expect(heatWeeks(0)).toBe(10);
    expect(heatWeeks(200)).toBe(11); // 11 columns of 14 plus 10 gaps of 4 is 194
    expect(heatWeeks(356)).toBe(20); // 20 columns of 14 plus 19 gaps of 4
    expect(heatWeeks(373)).toBe(20);
    expect(heatWeeks(374)).toBe(21);
    expect(heatWeeks(600)).toBe(33);
    expect(heatWeeks(5000)).toBe(52);
  });

  it("names a month on the column that holds its first day", () => {
    const from = localDay(new Date(2026, 8, 20));
    const name = (d: Date) => `${d.getMonth() + 1}月`;
    expect(monthLabels(from, 3, name)).toEqual(["", "10月", ""]);
    expect(monthLabels(localDay(new Date(2026, 9, 1)), 1, name)).toEqual(["10月"]);
  });
});
