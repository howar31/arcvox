import { afterEach, expect, test } from "vitest";
import { formatDateTime, formatDay, formatShortDate, formatTime, uiLocale } from "./format";
import { setLanguage } from "./i18n";

// 2026-03-05 14:07:09 local time, as seconds.
const ts = Math.floor(new Date(2026, 2, 5, 14, 7, 9).getTime() / 1000);

afterEach(() => setLanguage("en"));

test("dates follow the interface language, not the system locale", () => {
  setLanguage("zh-TW");
  expect(uiLocale()).toBe("zh-TW");
  expect(formatTime(ts)).toBe("14:07");
  expect(formatShortDate(ts)).toBe("3/5");
  expect(formatDateTime(ts)).toBe(new Date(ts * 1000).toLocaleString("zh-TW"));
  expect(formatDateTime(ts)).toContain("2026/3/5");

  setLanguage("en");
  expect(uiLocale()).toBe("en");
  expect(formatTime(ts)).toBe("14:07");
  expect(formatShortDate(ts)).toBe("3/5");
  expect(formatDateTime(ts)).toBe(new Date(ts * 1000).toLocaleString("en"));
  expect(formatDateTime(ts)).toContain("3/5/2026");
});

test("a day heading carries the year only when it is not the current one", () => {
  const day = new Date(2026, 2, 5);
  setLanguage("en");
  expect(formatDay(day, new Date(2026, 9, 22))).toBe("Thu, March 5");
  expect(formatDay(day, new Date(2027, 0, 1))).toBe("Thu, March 5, 2026");
  setLanguage("zh-TW");
  const sameYear = formatDay(day, new Date(2026, 9, 22));
  expect(sameYear).toContain("3月5日");
  expect(sameYear).not.toContain("2026");
  expect(formatDay(day, new Date(2027, 0, 1))).toContain("2026年3月5日");
});
