import { describe, expect, it } from "vitest";
import { diff, units } from "./textDiff";

const side = (changes: ReturnType<typeof diff>, drop: "del" | "ins") => changes.filter((c) => c.kind !== drop).map((c) => c.text).join("");

describe("text diff", () => {
  it("takes a Latin word as one unit and other text a character at a time", () => {
    expect(units("跟 Kevin 開會")).toEqual(["跟", " ", "Kevin", " ", "開", "會"]);
    expect(units("it's 4pm")).toEqual(["it's", " ", "4pm"]);
    expect(units("")).toEqual([]);
  });
  it("rebuilds both texts from the changes", () => {
    const pairs = [
      ["今天下午三點要跟 Kevin 開會 不對 是四點 記得帶 MacBook 和投影片", "今天下午四點要跟 Kevin 開會，記得帶 MacBook 和投影片。"],
      ["so um the meeting is at three no wait four", "The meeting is at four."],
      ["", "新的"],
      ["舊的", ""],
    ];
    for (const [from, to] of pairs) {
      const changes = diff(from, to);
      expect(side(changes, "ins")).toBe(from);
      expect(side(changes, "del")).toBe(to);
    }
  });
  it("keeps what did not change", () => {
    expect(diff("好的", "好的")).toEqual([{ kind: "keep", text: "好" }, { kind: "keep", text: "的" }]);
    const changes = diff("會議改到下週三 地點不變", "會議改到下週三，地點不變。");
    expect(changes.filter((c) => c.kind === "del").map((c) => c.text)).toEqual([" "]);
    expect(changes.filter((c) => c.kind === "ins").map((c) => c.text)).toEqual(["，", "。"]);
  });
});
