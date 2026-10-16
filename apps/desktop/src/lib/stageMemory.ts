// What the profiles page shows first: the profile and the stage it showed when it was left.

import { STAGE_IDS, type StageId } from "./flowLast";

const KEY = "arcvox.flow.stage";

/** The remembered stage, or the speech stage, the first one with settings. */
export function storedStage(): StageId {
  try {
    const v = localStorage.getItem(KEY);
    return STAGE_IDS.includes(v as StageId) ? (v as StageId) : "asr";
  } catch { return "asr"; }
}

export function rememberStage(stage: StageId): void {
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
