// What the profiles page shows first: the profile and the stage it showed when it was left.

import { CELL_IDS, type CellId } from "./flowLast";

const KEY = "arcvox.flow.stage";
// The cells that held the trim and the two conversions before they were folded into their stages.
const FOLDED: Record<string, CellId> = { trim: "capture", opencc1: "asr", opencc2: "llm" };

/** The remembered cell, or the speech stage, the first one with settings. */
export function storedStage(): CellId {
  try {
    const v = localStorage.getItem(KEY) ?? "";
    return CELL_IDS.includes(v as CellId) ? (v as CellId) : FOLDED[v] ?? "asr";
  } catch { return "asr"; }
}

export function rememberStage(stage: CellId): void {
  try { localStorage.setItem(KEY, stage); } catch { /* The stage is not remembered. */ }
}

const PROFILE_KEY = "arcvox.flow.profile";

/** The profile shown when the page was left; null when none was remembered. */
export function storedProfile(): string | null {
  try { return localStorage.getItem(PROFILE_KEY); } catch { return null; }
}

export function rememberProfile(id: string): void {
  try { localStorage.setItem(PROFILE_KEY, id); } catch { /* The profile is not remembered. */ }
}
