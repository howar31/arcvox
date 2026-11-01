import { describe, expect, it } from "vitest";
import { filterGroups, wantsSearch } from "./dropdown";

describe("wantsSearch", () => {
  it("offers a search from three items in a list that varies", () => {
    expect(wantsSearch(2, true)).toBe(false);
    expect(wantsSearch(3, true)).toBe(true);
  });
  it("offers a search from eight items in a fixed list", () => {
    expect(wantsSearch(7, false)).toBe(false);
    expect(wantsSearch(8, false)).toBe(true);
  });
});

describe("filterGroups", () => {
  const groups = [
    { label: "A", choices: [{ value: "1", label: "Breeze" }, { value: "2", label: "Whisper", help: "many languages" }] },
    { label: "B", choices: [{ value: "3", label: "Gemini" }] },
  ];
  it("keeps every group without a query", () => {
    expect(filterGroups(groups, "  ")).toBe(groups);
  });
  it("matches the label and the help, whatever the case", () => {
    expect(filterGroups(groups, "BREEZE").flatMap((g) => g.choices.map((c) => c.value))).toEqual(["1"]);
    expect(filterGroups(groups, "languages").flatMap((g) => g.choices.map((c) => c.value))).toEqual(["2"]);
  });
  it("leaves out a group with nothing left", () => {
    expect(filterGroups(groups, "gemini").map((g) => g.label)).toEqual(["B"]);
  });
});
