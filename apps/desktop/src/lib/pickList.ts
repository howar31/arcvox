// Ticking rows of a list: one at a time, a run of rows with Shift, or every row at once.

export interface Picked { selected: string[]; anchor: string | null }

/**
 * Ticks or unticks `id`. With `shift` and an earlier row as the anchor, every row from the anchor to
 * `id` takes the state `id` takes; rows that cannot be ticked stay as they are. The anchor moves to `id`
 * on a plain press only, so further Shift presses extend from the same row.
 */
export function pick(order: string[], state: Picked, id: string, shift: boolean, locked: (id: string) => boolean = () => false): Picked {
  if (locked(id)) return state;
  const on = !state.selected.includes(id);
  const from = state.anchor === null ? -1 : order.indexOf(state.anchor);
  const to = order.indexOf(id);
  if (!shift || from < 0 || to < 0) {
    return { selected: on ? [...state.selected, id] : state.selected.filter((x) => x !== id), anchor: id };
  }
  const run = order.slice(Math.min(from, to), Math.max(from, to) + 1).filter((x) => !locked(x));
  const rest = state.selected.filter((x) => !run.includes(x));
  return { selected: on ? [...rest, ...run] : rest, anchor: state.anchor };
}

/** The rows that can be ticked, in list order. */
export const pickable = (order: string[], locked: (id: string) => boolean = () => false): string[] => order.filter((x) => !locked(x));

/** How many of the tickable rows are ticked: none, some or all. */
export function coverage(order: string[], selected: string[], locked: (id: string) => boolean = () => false): "none" | "some" | "all" {
  const open = pickable(order, locked);
  const n = open.filter((x) => selected.includes(x)).length;
  return n === 0 ? "none" : n === open.length ? "all" : "some";
}

/** The box above the list: ticks every row that can be ticked, or clears them when all are. */
export function pickAll(order: string[], selected: string[], locked: (id: string) => boolean = () => false): string[] {
  const open = pickable(order, locked);
  return coverage(order, selected, locked) === "all" ? selected.filter((x) => !open.includes(x)) : [...new Set([...selected, ...open])];
}
