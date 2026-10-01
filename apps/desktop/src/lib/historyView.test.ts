import { describe, expect, it } from "vitest";
import type { Utterance } from "./api";
import { averageSeconds, groupByDay, seconds, showsRaw, totalSeconds } from "./historyView";

const at = (d: Date) => Math.floor(d.getTime() / 1000);
const u = (id: number, when: Date, raw = "raw", fin = "final", degraded = false): Utterance => ({
  id, ts: at(when), profile_id: "default", app_name: "Notes", raw_text: raw, refined_text: fin, final_text: fin,
  degraded, has_audio: false, asr_ms: 1180, llm_ms: degraded ? null : 940,
});

describe("groupByDay", () => {
  const now = new Date(2026, 8, 30, 0, 20);

  it("splits rows at local midnight, newest first", () => {
    const rows = [
      u(1, new Date(2026, 8, 27, 9, 0)),
      u(4, new Date(2026, 8, 30, 0, 5)),
      u(3, new Date(2026, 8, 29, 23, 55)),
      u(2, new Date(2026, 8, 29, 0, 0)),
    ];
    const groups = groupByDay(rows, now);
    expect(groups.map((g) => g.key)).toEqual(["today", "yesterday", "2026-09-27"]);
    expect(groups.map((g) => g.rows.map((r) => r.id))).toEqual([[4], [3, 2], [1]]);
  });

  it("uses the date for a day across a month and year boundary", () => {
    const groups = groupByDay([u(1, new Date(2025, 11, 31, 12, 0))], new Date(2026, 0, 2, 8, 0));
    expect(groups.map((g) => g.key)).toEqual(["2025-12-31"]);
    expect(groupByDay([u(1, new Date(2025, 11, 31, 23, 0))], new Date(2026, 0, 1, 1, 0))[0].key).toBe("yesterday");
  });

  it("returns nothing for no rows", () => {
    expect(groupByDay([], now)).toEqual([]);
  });
});

describe("seconds", () => {
  it("formats milliseconds as seconds with one decimal", () => {
    expect(seconds(1180)).toBe("1.2");
    expect(seconds(940)).toBe("0.9");
    expect(seconds(0)).toBe("0.0");
  });
  it("has nothing for a step that did not run", () => {
    expect(seconds(null)).toBeNull();
  });
});

describe("showsRaw", () => {
  it("shows the raw transcript only when it differs from the final text", () => {
    expect(showsRaw(u(1, new Date(), "好 我等一下看", "好，我等一下看。"))).toBe(true);
    expect(showsRaw(u(1, new Date(), "same text ", " same text"))).toBe(false);
  });
  it("hides it for a degraded row that inserted the raw text", () => {
    expect(showsRaw(u(1, new Date(), "好 我等一下看", "好 我等一下看", true))).toBe(false);
  });
  it("hides an empty raw transcript", () => {
    expect(showsRaw(u(1, new Date(), "  ", "typed"))).toBe(false);
  });
});

describe("totalSeconds", () => {
  it("adds recognition and rewriting before rounding", () => {
    expect(totalSeconds(1180, 940)).toBe("2.1");
    expect(totalSeconds(1140, 940)).toBe("2.1");
    expect(totalSeconds(60, 60)).toBe("0.1");
  });

  it("is null when the rewrite did not run: the recognition time is the total", () => {
    expect(totalSeconds(1180, null)).toBeNull();
  });
});

describe("averageSeconds", () => {
  it("divides the time by the number of dictations", () => {
    expect(averageSeconds({ count: 4, total_ms: 8600 })).toBe("2.1");
  });

  it("is null without dictations", () => {
    expect(averageSeconds({ count: 0, total_ms: 0 })).toBeNull();
  });
});
