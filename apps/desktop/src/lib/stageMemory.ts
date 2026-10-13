// The stage the profiles page shows first: the one it showed when it was left.

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
