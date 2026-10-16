// Where a list that opens under a button goes: below it when the whole list fits there, else on the
// side with more room, no taller than that room, so only the list scrolls and the page never does.

export interface Anchor { top: number; bottom: number; left: number; width: number }
export interface Placement {
  /** One of the two is set: the list hangs from `top` or rises from `bottom` (both from the viewport's top edge). */
  top: number | null;
  bottom: number | null;
  left: number;
  width: number;
  /** Null when the whole list fits. */
  maxHeight: number | null;
}

const GAP = 6;
const MARGIN = 8;

/**
 * `need` is the list's natural height; `viewport` the window's size; `minWidth` widens a narrow button's
 * list. A list that fits below the button hangs there whole; otherwise it takes the roomier side and
 * scrolls inside.
 */
export function placeMenu(anchor: Anchor, need: number, viewport: { width: number; height: number }, minWidth = 0): Placement {
  const below = viewport.height - anchor.bottom - GAP - MARGIN;
  const above = anchor.top - GAP - MARGIN;
  const width = Math.min(Math.max(anchor.width, minWidth), viewport.width - anchor.left - MARGIN);
  if (need <= below || below >= above) {
    return { top: anchor.bottom + GAP, bottom: null, left: anchor.left, width, maxHeight: need <= below ? null : Math.max(below, 0) };
  }
  return { top: null, bottom: viewport.height - anchor.top + GAP, left: anchor.left, width, maxHeight: need <= above ? null : above };
}

/**
 * Where a help card that opens from an ⓘ goes: under the mark, its left edge a little left of it, kept
 * inside the viewport; above the mark when there is no room below. `size` is the card's natural size.
 */
export function placeTip(anchor: Anchor, size: { width: number; height: number }, viewport: { width: number; height: number }): { top: number | null; bottom: number | null; left: number } {
  const left = Math.max(MARGIN, Math.min(anchor.left - MARGIN, viewport.width - size.width - MARGIN));
  const below = viewport.height - anchor.bottom - GAP - MARGIN;
  if (size.height <= below || below >= anchor.top - GAP - MARGIN) {
    return { top: anchor.bottom + GAP, bottom: null, left };
  }
  return { top: null, bottom: viewport.height - anchor.top + GAP, left };
}
