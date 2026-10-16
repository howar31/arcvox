// Which blocks of an expanded History row are open: remembered per block for the whole page.

const KEY = "arcvox.history.open";

export type DetailBlock = "stages" | "attempts";

/** Whether a block was left open; closed until the user opens it. */
export function storedOpen(block: DetailBlock): boolean {
  try { return localStorage.getItem(`${KEY}.${block}`) === "1"; } catch { return false; }
}

export function rememberOpen(block: DetailBlock, open: boolean): void {
  try { localStorage.setItem(`${KEY}.${block}`, open ? "1" : "0"); } catch { /* The state is not remembered. */ }
}
