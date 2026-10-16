import { describe, expect, it } from "vitest";
import { placeMenu, placeTip } from "./popover";

const view = { width: 1000, height: 800 };

describe("placeMenu", () => {
  it("hangs a list that fits below the button, whole", () => {
    const p = placeMenu({ top: 100, bottom: 144, left: 300, width: 500 }, 300, view);
    expect(p).toEqual({ top: 150, bottom: null, left: 300, width: 500, maxHeight: null });
  });
  it("limits a long list to the room below when that is the roomier side", () => {
    const p = placeMenu({ top: 100, bottom: 144, left: 300, width: 500 }, 900, view);
    expect(p.top).toBe(150);
    expect(p.maxHeight).toBe(800 - 144 - 14);
  });
  it("rises above a button low on the page", () => {
    const p = placeMenu({ top: 700, bottom: 744, left: 300, width: 500 }, 300, view);
    expect(p).toEqual({ top: null, bottom: 800 - 700 + 6, left: 300, width: 500, maxHeight: null });
  });
  it("scrolls inside when neither side has the room", () => {
    const p = placeMenu({ top: 500, bottom: 544, left: 300, width: 500 }, 900, view);
    expect(p.bottom).toBe(306);
    expect(p.maxHeight).toBe(500 - 14);
  });
  it("widens a narrow button's list but keeps it inside the window", () => {
    expect(placeMenu({ top: 100, bottom: 144, left: 300, width: 360 }, 100, view, 420).width).toBe(420);
    expect(placeMenu({ top: 100, bottom: 144, left: 700, width: 360 }, 100, view, 420).width).toBe(1000 - 700 - 8);
  });
});

describe("placeTip", () => {
  const vp = { width: 1000, height: 600 };
  it("hangs under the mark, a little to its left", () => {
    expect(placeTip({ top: 100, bottom: 120, left: 300, width: 20 }, { width: 320, height: 60 }, vp)).toEqual({ top: 126, bottom: null, left: 292 });
  });
  it("stays inside the viewport on the right and the left", () => {
    expect(placeTip({ top: 100, bottom: 120, left: 900, width: 20 }, { width: 320, height: 60 }, vp).left).toBe(672);
    expect(placeTip({ top: 100, bottom: 120, left: 2, width: 20 }, { width: 320, height: 60 }, vp).left).toBe(8);
  });
  it("rises above the mark when the room below is short", () => {
    expect(placeTip({ top: 560, bottom: 580, left: 300, width: 20 }, { width: 320, height: 60 }, vp)).toEqual({ top: null, bottom: 46, left: 292 });
  });
});

describe("tipKind", () => {
  it("keeps a short label on one line and wraps a longer text", async () => {
    const { tipKind } = await import("./tip");
    expect(tipKind("複製")).toBe("short");
    expect(tipKind("a".repeat(40))).toBe("short");
    expect(tipKind("a".repeat(41))).toBe("long");
    expect(tipKind("two\nlines")).toBe("long");
  });
});
