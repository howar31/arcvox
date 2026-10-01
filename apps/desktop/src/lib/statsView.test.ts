import { describe, expect, it } from "vitest";
import { durationParts, fillDays, heatLevel, localDay, percent, periodStart, speed, streaks, topWithOthers } from "./statsView";

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
    expect(fillDays([{ day: 3, count: 1, chars: 5 }], 2, 4)).toEqual([0, 5, 0]);
    expect(fillDays([], 4, 2)).toEqual([]);
    const rows = ["a", "b", "c"].map((name, i) => ({ name, count: 3 - i, chars: 0 }));
    expect(topWithOthers(rows, 2)).toEqual({ top: rows.slice(0, 2), others: { count: 1, names: 1 } });
    expect(topWithOthers(rows, 5).others).toBeNull();
  });

  it("splits a duration into hours, minutes and seconds", () => {
    expect(durationParts(0)).toEqual({ hours: 0, minutes: 0, seconds: 0 });
    expect(durationParts(2 * 3600_000 + 6 * 60_000 + 7_400)).toEqual({ hours: 2, minutes: 6, seconds: 7 });
  });
});
