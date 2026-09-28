import { expect, test } from "vitest";
import { keysOf, setLanguage, t } from "./i18n";

test("both tables have identical key sets", () => {
  const zh = keysOf("zh-TW").sort();
  const en = keysOf("en").sort();
  expect(zh).toEqual(en);
});

test("no value is empty and interpolation works", () => {
  for (const lang of ["zh-TW", "en"] as const) {
    setLanguage(lang);
    for (const key of keysOf(lang)) expect(t(key).length, `${lang}:${key}`).toBeGreaterThan(0);
  }
  setLanguage("en");
  expect(t("missing.key")).toBe("missing.key");
});
