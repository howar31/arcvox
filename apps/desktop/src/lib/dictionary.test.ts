import { describe, expect, it } from "vitest";
import { addAliases, dropBlank, hintCountLine, isSnippet, trimEntries } from "./dictionary";
import { setLanguage } from "./i18n";

it("trims terms and aliases so a trailing newline keeps a term a term", () => {
  expect(trimEntries([
    { term: "Arcvox\n", aliases: [" k ", " "], asr_hint: true },
    { term: "\nBest regards,\nThe team\n", aliases: ["sig"], asr_hint: false },
  ])).toEqual([
    { term: "Arcvox", aliases: ["k"], asr_hint: true },
    { term: "Best regards,\nThe team", aliases: ["sig"], asr_hint: false },
  ]);
});

describe("addAliases", () => {
  it("splits typed text on commas and spaces, both widths", () => {
    expect(addAliases([], "雅客, 亞克，k  w")).toEqual(["雅客", "亞克", "k", "w"]);
  });
  it("keeps existing aliases and skips duplicates and blanks", () => {
    expect(addAliases(["a"], " , a,b ")).toEqual(["a", "b"]);
  });
});

it("treats only a term with an inner line break as a snippet", () => {
  expect(isSnippet("Arcvox\n")).toBe(false);
  expect(isSnippet("\n Arcvox \n")).toBe(false);
  expect(isSnippet("Best regards,\nThe team")).toBe(true);
});

it("drops entries with neither a term nor an alias", () => {
  expect(dropBlank([
    { term: " \n", aliases: [], asr_hint: true },
    { term: "", aliases: ["k"], asr_hint: true },
    { term: "Arcvox", aliases: [], asr_hint: true },
  ])).toEqual([
    { term: "", aliases: ["k"], asr_hint: true },
    { term: "Arcvox", aliases: [], asr_hint: true },
  ]);
});

describe("hintCountLine", () => {
  it("says how many go with a dictation and the engine's limit", () => {
    setLanguage("zh-TW");
    expect(hintCountLine({ sent: 40, total: 40, limit_terms: 100, limit_bytes: null })).toBe("這個情境每次提示 40 個詞彙，上限 100 個。");
    expect(hintCountLine({ sent: 23, total: 40, limit_terms: null, limit_bytes: 600 })).toBe("這個情境每次提示 23 個詞彙，上限 600 bytes。");
  });
  it("has nothing to say without terms or a limit", () => {
    expect(hintCountLine({ sent: 0, total: 0, limit_terms: 100, limit_bytes: null })).toBe("");
    expect(hintCountLine({ sent: 0, total: 40, limit_terms: null, limit_bytes: null })).toBe("");
    expect(hintCountLine({ sent: 0, total: 40, limit_terms: 0, limit_bytes: null })).toBe("");
  });
});
