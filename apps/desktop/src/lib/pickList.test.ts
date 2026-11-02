import { describe, expect, it } from "vitest";
import { coverage, pick, pickAll } from "./pickList";

const order = ["a", "b", "c", "d", "e"];
const none = { selected: [], anchor: null };

describe("pick", () => {
  it("ticks and unticks one row and moves the anchor", () => {
    const one = pick(order, none, "b", false);
    expect(one).toEqual({ selected: ["b"], anchor: "b" });
    expect(pick(order, one, "b", false)).toEqual({ selected: [], anchor: "b" });
  });

  it("ticks the run from the anchor with Shift, in either direction", () => {
    const start = pick(order, none, "b", false);
    expect(pick(order, start, "d", true).selected.sort()).toEqual(["b", "c", "d"]);
    const back = pick(order, pick(order, none, "d", false), "a", true);
    expect(back.selected.sort()).toEqual(["a", "b", "c", "d"]);
    expect(back.anchor).toBe("d");
  });

  it("unticks the run when the pressed row was ticked", () => {
    const all = { selected: [...order], anchor: "a" };
    expect(pick(order, all, "c", true).selected.sort()).toEqual(["d", "e"]);
  });

  it("extends from the same anchor on further Shift presses", () => {
    const s1 = pick(order, pick(order, none, "c", false), "e", true);
    const s2 = pick(order, s1, "a", true);
    expect(s2.selected.sort()).toEqual(["a", "b", "c", "d", "e"]);
  });

  it("leaves locked rows alone", () => {
    const locked = (id: string) => id === "c";
    expect(pick(order, none, "c", false, locked)).toEqual(none);
    const run = pick(order, pick(order, none, "a", false, locked), "e", true, locked);
    expect(run.selected.sort()).toEqual(["a", "b", "d", "e"]);
  });

  it("acts as a plain press without an anchor", () => {
    expect(pick(order, none, "c", true)).toEqual({ selected: ["c"], anchor: "c" });
  });
});

describe("pickAll and coverage", () => {
  const locked = (id: string) => id === "a";
  it("ticks every tickable row, then clears them", () => {
    const all = pickAll(order, ["b"], locked);
    expect(all.sort()).toEqual(["b", "c", "d", "e"]);
    expect(coverage(order, all, locked)).toBe("all");
    expect(pickAll(order, all, locked)).toEqual([]);
  });
  it("tells none, some and all apart", () => {
    expect(coverage(order, [], locked)).toBe("none");
    expect(coverage(order, ["c"], locked)).toBe("some");
  });
});
