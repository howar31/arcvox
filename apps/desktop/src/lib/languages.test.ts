import { describe, expect, it } from "vitest";
import { setLanguage } from "./i18n";
import { languageName, shownLanguages } from "./languages";

describe("dictation languages", () => {
  it("names a tag in the interface language", () => {
    setLanguage("en");
    expect(languageName("ja-JP")).toContain("Japanese");
    expect(languageName("xx-unknown-tag-!!")).toBe("xx-unknown-tag-!!");
  });
  it("lists kept and used languages once each, not ones the system has from elsewhere", () => {
    setLanguage("en");
    expect(shownLanguages({ reserved: ["zh-TW", "fr-FR"] }, ["ja-JP", "zh-TW"]).sort()).toEqual(["fr-FR", "ja-JP", "zh-TW"]);
  });
});
