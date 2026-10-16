// How the Engines page shows its list of local models: the filter, and whether the models still
// to download are shown, as they were when the page was left.

export type ModelFilter = "all" | "installed" | "own";

const FILTER_KEY = "arcvox.engines.filter";
const FOLD_KEY = "arcvox.engines.downloadable";
const FILTERS: ModelFilter[] = ["all", "installed", "own"];

export function storedFilter(): ModelFilter {
  try {
    const v = localStorage.getItem(FILTER_KEY);
    return FILTERS.includes(v as ModelFilter) ? (v as ModelFilter) : "all";
  } catch { return "all"; }
}

export function rememberFilter(f: ModelFilter): void {
  try { localStorage.setItem(FILTER_KEY, f); } catch { /* The filter is not remembered. */ }
}

/** Whether the models to download were shown; null when the page has not been told yet. */
export function storedFold(): boolean | null {
  try {
    const v = localStorage.getItem(FOLD_KEY);
    return v === "open" ? true : v === "closed" ? false : null;
  } catch { return null; }
}

export function rememberFold(open: boolean): void {
  try { localStorage.setItem(FOLD_KEY, open ? "open" : "closed"); } catch { /* The fold is not remembered. */ }
}
