import type { StatsCard } from "./api";

/** The statistics page is a grid of three columns; every card is one or two rows tall and one to three columns wide. */
export const COLS = 3;
export const ROW_HEIGHT = 240;
export const GRID_GAP = 14;

export type CardId =
  | "t-chars" | "t-count" | "t-audio" | "t-saved"
  | "days" | "daily" | "apps" | "profiles" | "hours"
  | "speed" | "rewrite" | "time" | "lang" | "storage";

export interface CardDef { id: CardId; wmin: number; wmax: number; hmin: number; hmax: number }

/** Every card and the sizes it supports. */
export const CARDS: CardDef[] = [
  { id: "t-chars", wmin: 1, wmax: 2, hmin: 1, hmax: 1 },
  { id: "t-count", wmin: 1, wmax: 2, hmin: 1, hmax: 1 },
  { id: "t-audio", wmin: 1, wmax: 2, hmin: 1, hmax: 1 },
  { id: "t-saved", wmin: 1, wmax: 2, hmin: 1, hmax: 1 },
  // Seven rows of squares have one height, so the heatmap is never two rows tall.
  { id: "days", wmin: 1, wmax: 3, hmin: 1, hmax: 1 },
  { id: "daily", wmin: 1, wmax: 3, hmin: 1, hmax: 2 },
  { id: "apps", wmin: 1, wmax: 3, hmin: 1, hmax: 2 },
  { id: "profiles", wmin: 1, wmax: 3, hmin: 1, hmax: 2 },
  { id: "hours", wmin: 1, wmax: 3, hmin: 1, hmax: 2 },
  // Lists of figures have nothing more to show in a second row.
  { id: "speed", wmin: 1, wmax: 3, hmin: 1, hmax: 1 },
  { id: "rewrite", wmin: 1, wmax: 3, hmin: 1, hmax: 1 },
  { id: "time", wmin: 1, wmax: 3, hmin: 1, hmax: 1 },
  { id: "lang", wmin: 1, wmax: 3, hmin: 1, hmax: 2 },
  { id: "storage", wmin: 1, wmax: 3, hmin: 1, hmax: 1 },
];

export const DEFAULT_LAYOUT: StatsCard[] = [
  { id: "t-chars", row: 0, col: 0, w: 1, h: 1, hidden: false },
  { id: "t-count", row: 0, col: 1, w: 1, h: 1, hidden: false },
  { id: "t-audio", row: 0, col: 2, w: 1, h: 1, hidden: false },
  { id: "t-saved", row: 1, col: 0, w: 1, h: 1, hidden: false },
  { id: "days", row: 1, col: 1, w: 2, h: 1, hidden: false },
  { id: "daily", row: 2, col: 0, w: 3, h: 1, hidden: false },
  { id: "apps", row: 3, col: 0, w: 1, h: 2, hidden: false },
  { id: "profiles", row: 3, col: 1, w: 1, h: 1, hidden: false },
  { id: "hours", row: 3, col: 2, w: 1, h: 1, hidden: false },
  { id: "speed", row: 4, col: 1, w: 1, h: 1, hidden: false },
  { id: "rewrite", row: 4, col: 2, w: 1, h: 1, hidden: false },
  { id: "time", row: 5, col: 0, w: 1, h: 1, hidden: false },
  { id: "lang", row: 5, col: 1, w: 1, h: 1, hidden: false },
  { id: "storage", row: 5, col: 2, w: 1, h: 1, hidden: false },
];

export const cardDef = (id: string): CardDef | undefined => CARDS.find((c) => c.id === id);
export const supports = (def: CardDef, w: number, h: number) => w >= def.wmin && w <= def.wmax && h >= def.hmin && h <= def.hmax;

const shown = (layout: StatsCard[]) => layout.filter((c) => !c.hidden);
const covers = (c: StatsCard, row: number, col: number) => row >= c.row && row < c.row + c.h && col >= c.col && col < c.col + c.w;

/** The card occupying a cell, if any; `except` is left out of the search. */
export function occupant(layout: StatsCard[], row: number, col: number, except?: string): StatsCard | undefined {
  return shown(layout).find((c) => c.id !== except && covers(c, row, col));
}

/** Whether a card of size w × h can sit with its top left corner at (row, col). */
export function fits(layout: StatsCard[], row: number, col: number, w: number, h: number, except?: string): boolean {
  if (row < 0 || col < 0 || col + w > COLS) return false;
  for (let i = 0; i < h; i++) for (let j = 0; j < w; j++) if (occupant(layout, row + i, col + j, except)) return false;
  return true;
}

/** How many rows the shown cards reach. */
export function rowCount(layout: StatsCard[]): number {
  return Math.max(0, ...shown(layout).map((c) => c.row + c.h));
}

/** Where a card could take size w × h in its row while keeping its corner, else sliding left. */
export function placeFor(layout: StatsCard[], card: StatsCard, w: number, h: number): { row: number; col: number } | null {
  for (let dc = 0; dc <= COLS - 1; dc++) {
    const col = card.col - dc;
    if (fits(layout, card.row, col, w, h, card.id)) return { row: card.row, col };
  }
  return null;
}

/** Removes rows no card touches; cells left empty inside a row stay empty. */
export function collapseRows(layout: StatsCard[]): StatsCard[] {
  const out = layout.map((c) => ({ ...c }));
  for (let r = rowCount(out) - 1; r >= 0; r--) {
    if (shown(out).some((c) => r >= c.row && r < c.row + c.h)) continue;
    for (const c of out) if (!c.hidden && c.row > r) c.row--;
  }
  return out;
}

/** Whether a new row may be inserted above `row` without cutting a two-row card in half. */
export function canInsertRow(layout: StatsCard[], row: number, except?: string): boolean {
  return !shown(layout).some((c) => c.id !== except && c.row < row && c.row + c.h > row);
}

/**
 * Pushes the cards to the right of `card` further right so it can be w wide, keeping their order;
 * null when they would run off the edge or into another card.
 */
export function pushedRight(layout: StatsCard[], card: StatsCard, w: number, h: number): StatsCard[] | null {
  if (card.col + w > COLS) return null;
  const rowsOf = (c: StatsCard) => c.row < card.row + h && c.row + c.h > card.row;
  const group = shown(layout).filter((c) => c.id !== card.id && rowsOf(c) && c.col >= card.col + card.w).sort((a, b) => a.col - b.col);
  const moved = new Map<string, number>();
  let cursor = card.col + w;
  for (const n of group) {
    const col = Math.max(n.col, cursor);
    if (col + n.w > COLS) return null;
    moved.set(n.id, col);
    cursor = col + n.w;
  }
  const out = layout.map((c) => (c.id === card.id ? { ...c, w, h } : moved.has(c.id) ? { ...c, col: moved.get(c.id)! } : c));
  // Nothing may land on a card outside the group.
  for (const c of out) if (!c.hidden) for (let i = 0; i < c.h; i++) for (let j = 0; j < c.w; j++) {
    const o = out.find((x) => x.id !== c.id && !x.hidden && c.row + i >= x.row && c.row + i < x.row + x.h && c.col + j >= x.col && c.col + j < x.col + x.w);
    if (o) return null;
  }
  return out;
}

/**
 * Resizes a card to w × h. Sideways it first pushes the neighbours on its right into free cells,
 * then slides left when the right edge is in the way; downwards it pushes the rows below it down
 * to make room, unless a two-row card straddles that boundary. Unchanged when nothing works.
 */
export function resized(layout: StatsCard[], id: string, w: number, h: number): StatsCard[] {
  const card = layout.find((c) => c.id === id), def = cardDef(id);
  if (!card || !def || !supports(def, w, h)) return layout;
  if (fits(layout, card.row, card.col, w, h, id)) return collapseRows(layout.map((c) => (c.id === id ? { ...c, w, h } : c)));
  if (h <= card.h) {
    const pushed = pushedRight(layout, card, w, h);
    if (pushed) return collapseRows(pushed);
  }
  const at = placeFor(layout, card, w, h);
  if (at) return collapseRows(layout.map((c) => (c.id === id ? { ...c, ...at, w, h } : c)));
  if (h <= card.h) return layout;
  const below = card.row + card.h, extra = h - card.h;
  if (!canInsertRow(layout, below, id)) return layout;
  const pushed = layout.map((c) => (c.id !== id && !c.hidden && c.row >= below ? { ...c, row: c.row + extra } : c));
  const at2 = placeFor(pushed, card, w, h);
  if (!at2) return layout;
  return collapseRows(pushed.map((c) => (c.id === id ? { ...c, ...at2, w, h } : c)));
}

/** Whether a card may become w × h, by sliding or by pushing rows down. */
export function canResize(layout: StatsCard[], id: string, w: number, h: number): boolean {
  const card = layout.find((c) => c.id === id);
  if (!card) return false;
  if (card.w === w && card.h === h) return true;
  return resized(layout, id, w, h) !== layout;
}

/** Moves a card to a free place. */
export function movedTo(layout: StatsCard[], id: string, row: number, col: number): StatsCard[] {
  const card = layout.find((c) => c.id === id);
  if (!card || !fits(layout, row, col, card.w, card.h, id)) return layout;
  return collapseRows(layout.map((c) => (c.id === id ? { ...c, row, col, hidden: false } : c)));
}

/** Puts a card into a new row inserted above `row`; everything from that row down moves down. */
export function insertedAt(layout: StatsCard[], id: string, row: number, col: number): StatsCard[] {
  const card = layout.find((c) => c.id === id);
  if (!card || !canInsertRow(layout, row, id) || col < 0 || col + card.w > COLS) return layout;
  const out = layout.map((c) => (c.id !== id && !c.hidden && c.row >= row ? { ...c, row: c.row + card.h } : { ...c }));
  const me = out.find((c) => c.id === id)!;
  me.row = row; me.col = col; me.hidden = false;
  return collapseRows(out);
}

/** Exchanges the places of two shown cards of the same size. */
export function swapped(layout: StatsCard[], a: string, b: string): StatsCard[] {
  const ca = layout.find((c) => c.id === a), cb = layout.find((c) => c.id === b);
  if (!ca || !cb || ca.w !== cb.w || ca.h !== cb.h) return layout;
  return layout.map((c) => (c.id === a ? { ...c, row: cb.row, col: cb.col } : c.id === b ? { ...c, row: ca.row, col: ca.col } : c));
}

export function hidden(layout: StatsCard[], id: string): StatsCard[] {
  return collapseRows(layout.map((c) => (c.id === id ? { ...c, hidden: true } : c)));
}

/**
 * A usable layout from what the settings hold: every known card once, sizes within what the card
 * supports, no two cards overlapping. Anything else gives the built-in arrangement.
 */
export function sanitized(raw: StatsCard[] | null | undefined): StatsCard[] {
  if (!raw || !Array.isArray(raw)) return DEFAULT_LAYOUT;
  const out: StatsCard[] = [];
  for (const def of CARDS) {
    const c = raw.find((x) => x && x.id === def.id);
    if (!c) return DEFAULT_LAYOUT;
    const card = { id: def.id, row: c.row | 0, col: c.col | 0, w: c.w | 0, h: c.h | 0, hidden: !!c.hidden };
    if (!card.hidden && (!supports(def, card.w, card.h) || card.row < 0 || card.col < 0 || card.col + card.w > COLS)) return DEFAULT_LAYOUT;
    if (!card.hidden && !fits(out, card.row, card.col, card.w, card.h)) return DEFAULT_LAYOUT;
    if (card.hidden && !supports(def, card.w, card.h)) { card.w = def.wmin; card.h = def.hmin; }
    out.push(card);
  }
  return collapseRows(out);
}

export const sameLayout = (a: StatsCard[], b: StatsCard[]) => JSON.stringify(a) === JSON.stringify(b);
