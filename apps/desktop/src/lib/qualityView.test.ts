import { describe, expect, test } from "vitest";
import type { EvalItem, ReportHead } from "./api";
import { clock, comparable, fraction, perRow, providersOf, rate, worstFirst } from "./qualityView";

const item = (id: string, provider: string, text: [number, number], punct: [number, number], error: string | null = null): EvalItem => ({
  id, stage: "llm", provider, tags: [], expected: "", output: "", error, ms: 10,
  text: { errors: text[0], length: text[1] }, punctuation: { errors: punct[0], length: punct[1] }, checks: null,
});

describe("rates", () => {
  test("a rate is a percentage with one decimal and an empty expectation is zero", () => {
    expect(rate({ errors: 3, length: 100 })).toBe("3.0");
    expect(rate({ errors: 1, length: 3 })).toBe("33.3");
    expect(rate({ errors: 0, length: 0 })).toBe("0.0");
  });
  test("seconds per row", () => {
    expect(perRow({ ms_total: 3200, items: 2 })).toBe("1.6");
    expect(perRow({ ms_total: 0, items: 0 })).toBe("0.0");
  });
});

describe("worstFirst", () => {
  test("keeps one engine's rows, failures first, then the worse word rate, then the worse punctuation", () => {
    const items = [
      item("a", "g", [0, 10], [1, 2]),
      item("b", "g", [2, 10], [0, 2]),
      item("c", "other", [9, 10], [2, 2]),
      item("d", "g", [0, 10], [2, 2]),
      item("e", "g", [0, 10], [0, 2], "boom"),
    ];
    expect(worstFirst(items, "g").map((it) => it.id)).toEqual(["e", "b", "d", "a"]);
  });
});

describe("labels", () => {
  test("a full-stage label names both engines; a plain label is one", () => {
    expect(providersOf("breeze+gemma", "full")).toEqual(["breeze", "gemma"]);
    expect(providersOf("breeze+none", "full")).toEqual(["breeze"]);
    expect(providersOf("gemma", "llm")).toEqual(["gemma"]);
  });
  test("a clock shows minutes and seconds", () => {
    expect(clock(0)).toBe("0:00");
    expect(clock(192_400)).toBe("3:12");
  });
  test("the fraction done never passes one", () => {
    expect(fraction(3, 12)).toBe(0.25);
    expect(fraction(5, 0)).toBe(0);
    expect(fraction(14, 12)).toBe(1);
  });
});

describe("comparable", () => {
  const head = (file: string, stage: ReportHead["stage"]): ReportHead => ({ file, name: file, created: "1", origin: "app", stopped: false, stage, providers: [], items: 1, failed: 0, text: { errors: 0, length: 1 }, punctuation: { errors: 0, length: 1 } });
  test("only reports of the same stage, the report itself left out", () => {
    const heads = [head("a", "llm"), head("b", "asr"), head("c", "llm")];
    expect(comparable(heads, heads[0]).map((h) => h.file)).toEqual(["c"]);
    expect(comparable(heads, null)).toHaveLength(3);
  });
});
