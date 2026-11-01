import { describe, expect, it } from "vitest";
import type { Stage } from "./api";
import { wordPieces, wordsOf } from "./wordTimes";

const w = (text: string, start_ms: number, end_ms: number) => ({ text, start_ms, end_ms });

describe("word times", () => {
  it("cuts the text at the words and keeps what lies between", () => {
    const pieces = wordPieces("你好 smart。", [w("你", 0, 100), w("好", 100, 200), w("smart", 300, 600)]);
    expect(pieces).toEqual([
      { text: "你", word: w("你", 0, 100) },
      { text: "好", word: w("好", 100, 200) },
      { text: " ", word: null },
      { text: "smart", word: w("smart", 300, 600) },
      { text: "。", word: null },
    ]);
    expect(pieces.map((p) => p.text).join("")).toBe("你好 smart。");
  });

  it("passes over a word the text does not have", () => {
    expect(wordPieces("你好", [w("甲", 0, 100), w("好", 100, 200)])).toEqual([
      { text: "你", word: null },
      { text: "好", word: w("好", 100, 200) },
    ]);
  });

  it("shows the converted text when it keeps the transcript's length", () => {
    const words = [w("这", 0, 100), w("个", 100, 200)];
    expect(wordPieces("这个。", words, "這個。").map((p) => p.text)).toEqual(["這", "個", "。"]);
    expect(wordPieces("这个。", words, "這一個。").map((p) => p.text)).toEqual(["这", "个", "。"]);
  });

  it("gives the text alone without words", () => {
    expect(wordPieces("你好", [])).toEqual([{ text: "你好", word: null }]);
  });

  it("reads the words of the speech stage", () => {
    const stages: Stage[] = [
      { position: 1, kind: "capture", text: null, ms: 0, detail: {} },
      { position: 3, kind: "asr", text: "你", ms: 10, detail: { words: [w("你", 0, 100)] } },
    ];
    expect(wordsOf(stages)).toEqual([w("你", 0, 100)]);
    expect(wordsOf(null)).toEqual([]);
    expect(wordsOf([{ position: 3, kind: "asr", text: "你", ms: 10, detail: {} }])).toEqual([]);
  });
});
