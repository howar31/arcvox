import { expect, test } from "vitest";
import { has, keysOf, setLanguage, t } from "./i18n";

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
  expect(t("home.apps_more", { names: "A, B", count: "3" })).toBe("A, B and 3 more");
  expect(t("home.apps_more", { names: "A" })).toBe("A and {count} more");
});

test("has tells whether a key is translated", () => {
  for (const lang of ["zh-TW", "en"] as const) {
    setLanguage(lang);
    expect(has("common.save")).toBe(true);
    expect(has("missing.key")).toBe(false);
  }
  setLanguage("en");
});
