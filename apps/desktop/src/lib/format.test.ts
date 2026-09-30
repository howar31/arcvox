import { afterEach, expect, test } from "vitest";
import { formatDateTime, formatShortDate, formatTime, uiLocale } from "./format";
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
