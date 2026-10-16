import { describe, expect, it } from "vitest";
import { placeMenu } from "./popover";

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
