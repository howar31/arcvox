import { describe, expect, it } from "vitest";
import type { Utterance } from "./api";
import { clipLength, corrected, groupByDay, periodLabel, periodRange, rawState, redoneTag, seconds, spentSeconds, totalSeconds } from "./historyView";
import { setLanguage } from "./i18n";

const at = (d: Date) => Math.floor(d.getTime() / 1000);
const u = (id: number, when: Date, raw = "raw", fin = "final", degraded = false): Utterance => ({
  id, ts: at(when), profile_id: "default", app_name: "Notes", raw_text: raw, refined_text: fin, final_text: fin,
  degraded, has_audio: false, asr_ms: 1180, llm_ms: degraded ? null : 940, audio_ms: null, audio_bytes: null, error: null, error_code: null, redone: 0,
});

describe("redoneTag", () => {
  it("names once or how many times", () => {
    setLanguage("zh-TW");
    expect(redoneTag(0)).toBeNull();
    expect(redoneTag(1)).toBe("已重新處理");
    expect(redoneTag(3)).toBe("已重新處理 3 次");
    setLanguage("en");
    expect(redoneTag(2)).toBe("Processed again 2 times");
  });
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

describe("rawState", () => {
  it("tells a changed transcript from one the rewrite left as it was", () => {
    expect(rawState(u(1, new Date(), "好 我等一下看", "好，我等一下看。"))).toBe("changed");
    expect(rawState(u(1, new Date(), "same text ", " same text"))).toBe("same");
  });
  it("tells a failed rewrite from a profile that does not rewrite", () => {
    expect(rawState(u(1, new Date(), "好 我等一下看", "好 我等一下看", true))).toBe("failed");
    expect(rawState({ ...u(1, new Date(), "好", "好"), llm_ms: null })).toBe("off");
  });
  it("reports an empty raw transcript as absent", () => {
    expect(rawState(u(1, new Date(), "  ", "typed"))).toBe("none");
  });
  it("compares the transcript with what the app put in, not with the user's correction", () => {
    expect(rawState({ ...u(1, new Date(), "好 我等一下看", "好 我等一下看"), final_text: "好，我等一下看" })).toBe("same");
  });
});

describe("corrected", () => {
  it("marks a row whose final text differs from the text put in", () => {
    expect(corrected({ ...u(1, new Date(), "测试一下", "測試一下，停歇"), final_text: "測試一下聽寫" })).toBe(true);
    expect(corrected(u(1, new Date(), "测试一下", "測試一下，停歇"))).toBe(false);
    expect(corrected({ ...u(1, new Date(), "x", "y"), final_text: "z", error: "failed", error_code: "provider_timeout" })).toBe(false);
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

describe("clipLength", () => {
  it("shows minutes and two-digit seconds", () => {
    expect(clipLength(6400)).toBe("0:06");
    expect(clipLength(161000)).toBe("2:41");
    expect(clipLength(0)).toBe("0:00");
  });
});

describe("periodRange", () => {
  const now = new Date(2026, 9, 8, 15, 30);
  const sec = (d: Date) => Math.floor(d.getTime() / 1000);

  it("has no bounds for all", () => {
    expect(periodRange({ kind: "all" }, now)).toEqual({ since: null, until: null });
  });

  it("covers today or yesterday as one local day", () => {
    expect(periodRange({ kind: "today" }, now)).toEqual({ since: sec(new Date(2026, 9, 8)), until: null });
    expect(periodRange({ kind: "yesterday" }, now)).toEqual({ since: sec(new Date(2026, 9, 7)), until: sec(new Date(2026, 9, 8)) });
  });

  it("counts the last 7 and 30 days including today", () => {
    expect(periodRange({ kind: "7d" }, now)).toEqual({ since: sec(new Date(2026, 9, 2)), until: null });
    expect(periodRange({ kind: "30d" }, now)).toEqual({ since: sec(new Date(2026, 8, 9)), until: null });
  });

  it("takes a range of whole local days, and hours inside a single day", () => {
    expect(periodRange({ kind: "range", from: "2026-09-29", to: "2026-10-03" }, now)).toEqual({ since: sec(new Date(2026, 8, 29)), until: sec(new Date(2026, 9, 4)) });
    expect(periodRange({ kind: "range", from: "2026-10-03", to: "2026-10-03" }, now)).toEqual({ since: sec(new Date(2026, 9, 3)), until: sec(new Date(2026, 9, 4)) });
    expect(periodRange({ kind: "range", from: "2026-10-03", to: "2026-10-03", hourFrom: 14, hourTo: 16 }, now)).toEqual({ since: sec(new Date(2026, 9, 3, 14)), until: sec(new Date(2026, 9, 3, 17)) });
    // Hours are ignored over more than one day.
    expect(periodRange({ kind: "range", from: "2026-10-02", to: "2026-10-03", hourFrom: 14, hourTo: 16 }, now)).toEqual({ since: sec(new Date(2026, 9, 2)), until: sec(new Date(2026, 9, 4)) });
  });
});

describe("periodLabel", () => {
  it("names the selection in words", () => {
    setLanguage("zh-TW");
    expect(periodLabel({ kind: "all" })).toBe("全部");
    expect(periodLabel({ kind: "7d" })).toBe("最近 7 天");
    expect(periodLabel({ kind: "range", from: "2026-09-29", to: "2026-10-03" })).toBe("9/29 到 10/3");
    expect(periodLabel({ kind: "range", from: "2026-10-03", to: "2026-10-03" })).toBe("10/3");
    expect(periodLabel({ kind: "range", from: "2026-10-03", to: "2026-10-03", hourFrom: 14, hourTo: 16 })).toBe("10/3 14:00 到 17:00");
    setLanguage("en");
    expect(periodLabel({ kind: "yesterday" })).toBe("Yesterday");
  });
});

describe("spentSeconds", () => {
  it("adds the rewrite when it ran and is recognition alone otherwise", () => {
    expect(spentSeconds(1200, 1100)).toBe("2.3");
    expect(spentSeconds(1200, null)).toBe("1.2");
  });
});
