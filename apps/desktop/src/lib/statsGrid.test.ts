import { describe, expect, it } from "vitest";
import {
  canInsertRow, canResize, CARDS, collapseRows, DEFAULT_LAYOUT, fits, hidden, insertedAt, movedTo, occupant, placeFor, pushedRight, resized, rowCount, sanitized, supports, swapped,
} from "./statsGrid";

const at = (id: string, layout = DEFAULT_LAYOUT) => layout.find((c) => c.id === id)!;

describe("statsGrid", () => {
  it("has a default layout with every card once and no overlaps", () => {
    expect(DEFAULT_LAYOUT.map((c) => c.id).sort()).toEqual(CARDS.map((c) => c.id).sort());
    expect(sanitized(DEFAULT_LAYOUT)).toEqual(DEFAULT_LAYOUT);
    for (const c of DEFAULT_LAYOUT) expect(supports(CARDS.find((d) => d.id === c.id)!, c.w, c.h)).toBe(true);
  });

  it("knows which card covers a cell and where a size fits", () => {
    expect(occupant(DEFAULT_LAYOUT, 1, 2)?.id).toBe("days");
    expect(occupant(DEFAULT_LAYOUT, 4, 0)?.id).toBe("apps"); // two rows tall
    expect(occupant(DEFAULT_LAYOUT, 6, 0)).toBeUndefined();
    expect(fits(DEFAULT_LAYOUT, 6, 0, 3, 1)).toBe(true);
    expect(fits(DEFAULT_LAYOUT, 0, 2, 2, 1)).toBe(false); // past the right edge
    expect(fits(DEFAULT_LAYOUT, 1, 1, 2, 1, "days")).toBe(true); // its own place, left out
    expect(rowCount(DEFAULT_LAYOUT)).toBe(6);
  });

  it("resizes in place, slides left when the edge is in the way, and refuses what cannot fit sideways", () => {
    // 時間 at (5, 0) cannot grow to the right: 語言 is at (5, 1).
    expect(resized(DEFAULT_LAYOUT, "time", 2, 1)).toBe(DEFAULT_LAYOUT);
    // 儲存空間 at (5, 2): two columns would run past the edge, and (5, 1) is 語言, so no.
    expect(resized(DEFAULT_LAYOUT, "storage", 2, 1)).toBe(DEFAULT_LAYOUT);
    // Hide 語言, then 儲存空間 can take two columns by sliding left.
    const l = hidden(DEFAULT_LAYOUT, "lang");
    const r = resized(l, "storage", 2, 1);
    expect(at("storage", r)).toMatchObject({ row: 5, col: 1, w: 2, h: 1 });
    // A size the card does not support is refused.
    expect(resized(DEFAULT_LAYOUT, "days", 1, 2)).toBe(DEFAULT_LAYOUT);
    expect(canResize(DEFAULT_LAYOUT, "days", 1, 2)).toBe(false);
    expect(canResize(DEFAULT_LAYOUT, "days", 2, 1)).toBe(true); // its own size
    expect(placeFor(DEFAULT_LAYOUT, at("t-saved"), 1, 1)).toEqual({ row: 1, col: 0 });
  });

  it("grows sideways by pushing the neighbours on its right into free cells", () => {
    // Row 5 is 時間, 語言, 儲存空間: full, so 時間 cannot widen.
    expect(resized(DEFAULT_LAYOUT, "time", 2, 1)).toBe(DEFAULT_LAYOUT);
    // Without 儲存空間, 時間 becomes two wide and 語言 moves to the last column.
    const l = hidden(DEFAULT_LAYOUT, "storage");
    const r = resized(l, "time", 2, 1);
    expect(at("time", r)).toMatchObject({ row: 5, col: 0, w: 2 });
    expect(at("lang", r)).toMatchObject({ row: 5, col: 2 });
    // 聽寫速度 at (4, 1) next to 改寫與修正 at (4, 2): nothing to the right can give way, so it slides left? (4, 0) is 依應用程式: no.
    expect(resized(DEFAULT_LAYOUT, "speed", 2, 1)).toBe(DEFAULT_LAYOUT);
    expect(pushedRight(DEFAULT_LAYOUT, at("t-chars"), 2, 1)).toBeNull(); // the first row is full
  });

  it("grows downwards by pushing the rows below down", () => {
    // 每日輸出字數 spans row 2; two rows tall pushes rows 3 and later down by one.
    const r = resized(DEFAULT_LAYOUT, "daily", 3, 2);
    expect(at("daily", r)).toMatchObject({ row: 2, h: 2 });
    expect(at("apps", r)).toMatchObject({ row: 4 });
    expect(at("storage", r)).toMatchObject({ row: 6 });
    // 依情境 at (3, 1) sits beside 依應用程式, which is two rows tall from row 3: a row cannot be inserted at 4.
    expect(resized(DEFAULT_LAYOUT, "profiles", 1, 2)).toBe(DEFAULT_LAYOUT);
    expect(canResize(DEFAULT_LAYOUT, "profiles", 1, 2)).toBe(false);
    // The last row grows into empty space without pushing anything.
    const last = resized(DEFAULT_LAYOUT, "lang", 1, 2);
    expect(at("lang", last)).toMatchObject({ row: 5, h: 2 });
    expect(at("storage", last)).toMatchObject({ row: 5 });
  });

  it("moves to free cells, swaps equal sizes, and inserts rows", () => {
    const moved = movedTo(DEFAULT_LAYOUT, "storage", 6, 0);
    expect(at("storage", moved)).toMatchObject({ row: 6, col: 0 });
    expect(movedTo(DEFAULT_LAYOUT, "storage", 0, 0)).toBe(DEFAULT_LAYOUT); // occupied
    const sw = swapped(DEFAULT_LAYOUT, "t-chars", "t-saved");
    expect(at("t-chars", sw)).toMatchObject({ row: 1, col: 0 });
    expect(at("t-saved", sw)).toMatchObject({ row: 0, col: 0 });
    expect(swapped(DEFAULT_LAYOUT, "t-chars", "days")).toBe(DEFAULT_LAYOUT); // different sizes
    // Inserting a row above row 4 would cut 依應用程式 (rows 3 and 4) in half.
    expect(canInsertRow(DEFAULT_LAYOUT, 4)).toBe(false);
    expect(canInsertRow(DEFAULT_LAYOUT, 3)).toBe(true);
    const ins = insertedAt(DEFAULT_LAYOUT, "storage", 3, 0);
    expect(at("storage", ins)).toMatchObject({ row: 3, col: 0 });
    expect(at("apps", ins)).toMatchObject({ row: 4 });
    expect(at("time", ins)).toMatchObject({ row: 6 });
  });

  it("collapses rows nothing touches and keeps empty cells", () => {
    const gap = DEFAULT_LAYOUT.map((c) => (c.row >= 2 ? { ...c, row: c.row + 2 } : c));
    const out = collapseRows(gap);
    expect(at("daily", out).row).toBe(2);
    // Hiding the only card of row 0 ... row 0 still has two cards, so nothing moves.
    expect(rowCount(hidden(DEFAULT_LAYOUT, "t-chars"))).toBe(6);
    expect(occupant(hidden(DEFAULT_LAYOUT, "t-chars"), 0, 0)).toBeUndefined();
  });

  it("falls back to the default layout when the stored one is unusable", () => {
    expect(sanitized(null)).toEqual(DEFAULT_LAYOUT);
    expect(sanitized([])).toEqual(DEFAULT_LAYOUT);
    const overlap = DEFAULT_LAYOUT.map((c) => (c.id === "t-count" ? { ...c, col: 0 } : c));
    expect(sanitized(overlap)).toEqual(DEFAULT_LAYOUT);
    const tooWide = DEFAULT_LAYOUT.map((c) => (c.id === "days" ? { ...c, h: 2 } : c));
    expect(sanitized(tooWide)).toEqual(DEFAULT_LAYOUT);
    const fine = hidden(DEFAULT_LAYOUT, "storage");
    expect(sanitized(JSON.parse(JSON.stringify(fine)))).toEqual(fine);
  });
});
