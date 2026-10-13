import { describe, expect, it } from "vitest";
import { lineDiff } from "./lineDiff";

describe("line diff", () => {
  it("marks changed lines and keeps the rest", () => {
    const from = "規則：\n1. 甲\n2. 乙\n3. 丙";
    const to = "規則：\n1. 甲\n2. 乙改\n3. 丙";
    expect(lineDiff(from, to)).toEqual([
      { kind: "keep", text: "規則：" },
      { kind: "keep", text: "1. 甲" },
      { kind: "del", text: "2. 乙" },
      { kind: "ins", text: "2. 乙改" },
      { kind: "keep", text: "3. 丙" },
    ]);
  });
  it("rebuilds both texts from the lines", () => {
    const from = "a\nb\nc\n";
    const to = "b\nc\nd";
    const lines = lineDiff(from, to);
    expect(lines.filter((l) => l.kind !== "ins").map((l) => l.text).join("\n")).toBe("a\nb\nc\n");
    expect(lines.filter((l) => l.kind !== "del").map((l) => l.text).join("\n")).toBe("b\nc\nd");
  });
  it("is all kept when the texts are the same", () => {
    expect(lineDiff("x", "x").every((l) => l.kind === "keep")).toBe(true);
  });
});
