import { describe, expect, it, vi } from "vitest";
import type { DictionaryEntry, TermIndex } from "./api";
import { arrange, matches, scriptOf, storedView, type Labels } from "./dictionaryView";

const e = (term: string, aliases: string[] = [], extra: Partial<DictionaryEntry> = {}): DictionaryEntry => ({ term, aliases, asr_hint: true, ...extra });
const labels: Labels = { strokes: (n) => `${n} 劃`, latin: "英文", kana: "假名", other: "其他", unknown: "其他中文", manual: "手動加入", suggestion: "從建議加入", snippet: "片段", term: "詞彙" };
const idx = (strokes: number | null, zhuyin: string | null, pinyin: string | null): TermIndex => ({ strokes, zhuyin, pinyin });

describe("matches", () => {
  it("finds the query in the term or an alias, ignoring case and spacing", () => {
    expect(matches(e("Zephyr Kit", ["zephyrkit"]), "zephyr kit")).toBe(true);
    expect(matches(e("權限", ["全線"]), "全線")).toBe(true);
    expect(matches(e("權限", ["全線"]), "程序")).toBe(false);
    expect(matches(e("權限"), "")).toBe(true);
  });
});

describe("scriptOf", () => {
  it("looks at the first character", () => {
    expect(scriptOf("Slack")).toBe("latin");
    expect(scriptOf(" 權限")).toBe("han");
    expect(scriptOf("カタカナ")).toBe("kana");
    expect(scriptOf("123")).toBe("other");
  });
});

describe("arrange", () => {
  const items = [e("權限", ["全線"], { added: 10 }), e("Slack", ["Slake", "史萊克"], { added: 30, origin: "suggestion" }), e("封包", [], { added: 20 }), e("Arcvox", ["阿克沃斯"], { updated: 40 }), e("Best regards,\nThe team", ["簽名檔"])];
  const index = [idx(21, "ㄑ", "Q"), null, idx(9, "ㄈ", "F"), null, null];

  it("puts the newest first and entries without a time after them in their stored order", () => {
    expect(arrange(items, "added", index, labels)[0].items.map((x) => x.term)).toEqual(["Slack", "封包", "權限", "Arcvox", "Best regards,\nThe team"]);
  });
  it("groups Latin terms by letter and Han terms by strokes, kana and the rest last", () => {
    const secs = arrange([...items, e("カタカナ")], "strokes", [...index, null], labels);
    expect(secs.map((s) => s.label)).toEqual(["A", "B", "S", "9 劃", "21 劃", "假名"]);
  });
  it("joins Han terms into the letters under pinyin and keeps Bopomofo in its own order", () => {
    expect(arrange(items, "pinyin", index, labels).map((s) => s.label)).toEqual(["A", "B", "F", "Q", "S"]);
    const hits = new Map([["Slack", { term: "Slack", count: 3, last: 10 }], ["權限", { term: "權限", count: 3, last: 20 }], ["封包", { term: "封包", count: 1, last: 5 }]]);
    expect(arrange(items, "hits", index, labels, hits)[0].items.map((x) => x.term)).toEqual(["權限", "Slack", "封包", "Arcvox", "Best regards,\nThe team"]);
    expect(arrange(items, "zhuyin", index, labels).map((s) => s.label)).toEqual(["A", "B", "S", "ㄈ", "ㄑ"]);
  });
  it("orders by alias count, origin and kind", () => {
    expect(arrange(items, "aliases", index, labels)[0].items[0].term).toBe("Slack");
    expect(arrange(items, "origin", index, labels).map((s) => [s.label, s.items.length])).toEqual([["從建議加入", 1], ["手動加入", 4]]);
    expect(arrange(items, "kind", index, labels).map((s) => [s.label, s.items.length])).toEqual([["詞彙", 4], ["片段", 1]]);
  });
});

describe("storedView", () => {
  it("opens the list for the two-pane view kept by an earlier version", () => {
    const kept = new Map<string, string>();
    vi.stubGlobal("localStorage", { getItem: (k: string) => kept.get(k) ?? null, setItem: (k: string, v: string) => kept.set(k, v) });
    kept.set("dictionary.view", "panes");
    expect(storedView()).toBe("list");
    kept.set("dictionary.view", "list");
    expect(storedView()).toBe("list");
    kept.delete("dictionary.view");
    expect(storedView()).toBe("cards");
    vi.unstubAllGlobals();
  });
});
