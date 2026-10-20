/** The steps a dictation's text went through, for the History page: the transcript, every stage that changed
 * the text, where it was put, and the user's correction read back from the field. Stages come from the
 * records kept for the row; rows whose stage records are gone show one processing step between the
 * transcript and the text put in. */
import type { Stage, Utterance } from "./api";
import { changedStages, cellLine, insertLines, inputOf, stageAt, stageName, type StageId } from "./flowLast";
import { corrected, rawState } from "./historyView";
import { t } from "./i18n";
import { diff } from "./textDiff";

export type FlowStep =
  /** The transcript, shown whole. */
  | { kind: "first"; mark: string; name: string; small: string; text: string }
  /** A stage that changed the text; `from` is what it received. */
  | { kind: "stage"; mark: string; name: string; small: string; from: string; text: string }
  /** A stage that failed; `text` is its reason. */
  | { kind: "failed"; mark: string; name: string; small: string; text: string }
  /** The text left the app: no text of its own. */
  | { kind: "mark"; name: string; small: string }
  /** The user's correction, against the text put in. */
  | { kind: "user"; name: string; small: string; from: string; text: string };

/** How many runs of changed text separate the two. */
export function changeCount(from: string, to: string): number {
  let runs = 0;
  let inRun = false;
  for (const c of diff(from, to)) {
    if (c.kind === "keep") inRun = false;
    else if (!inRun) { inRun = true; runs++; }
  }
  return runs;
}

const TEXT_STAGES: StageId[] = ["opencc1", "llm", "opencc2", "rules"];

/** The steps, and a line naming the stages that left the text alone (empty when none did). */
export function textFlow(u: Utterance, stages: Stage[] | null): { steps: FlowStep[]; unchanged: string } {
  const steps: FlowStep[] = [];
  const raw = u.raw_text.trim();
  const refined = u.refined_text.trim();
  const app = u.app_name ?? "";
  const kept = stages !== null && stages.length > 0;
  if (raw !== "") {
    const asr = kept ? stageAt(stages, "asr") : undefined;
    steps.push({ kind: "first", mark: "3", name: stageName("asr"), small: asr ? cellLine("asr", stages) : "", text: raw });
  }
  let unchanged = "";
  if (kept) {
    const changed = changedStages(stages);
    for (const id of changed) {
      const s = stageAt(stages, id)!;
      const name = stageName(id);
      const small = cellLine(id, stages);
      if (s.text === null) {
        const reason = typeof s.detail.message === "string" ? s.detail.message : typeof s.detail.error === "string" ? s.detail.error : "";
        steps.push({ kind: "failed", mark: String(s.position), name, small, text: reason });
      } else {
        steps.push({ kind: "stage", mark: String(s.position), name, small, from: inputOf(stages, id) ?? "", text: s.text });
      }
    }
    // A stage that left the text alone is named, unless a stage of the same name (the second script
    // conversion) did change it.
    const changedNames = new Set(changed.map(stageName));
    const left = [...new Set(TEXT_STAGES.filter((id) => stageAt(stages, id) && !changed.includes(id)).map(stageName))].filter((n) => !changedNames.has(n));
    if (left.length) unchanged = t("history.flow_unchanged", { names: left.join(t("common.list_sep")) });
    const insert = stageAt(stages, "insert");
    if (insert) {
      const [method, outcome] = insertLines(stages);
      const where = typeof insert.detail.app_name === "string" ? insert.detail.app_name : app;
      const copied = insert.detail.outcome === "copied";
      steps.push({ kind: "mark", name: copied ? t("history.flow_copied") : where ? t("history.flow_put_in", { app: where }) : t("history.flow_put_in_noapp"), small: copied ? "" : insert.detail.outcome === "inserted" || insert.detail.outcome === undefined ? method : outcome });
    }
  } else {
    if (raw !== "" && refined !== "" && raw !== refined) {
      steps.push({ kind: "stage", mark: "…", name: t("history.flow_processed"), small: t("history.flow_stages_gone"), from: raw, text: refined });
    } else {
      const state = rawState(u);
      unchanged = state === "same" ? t("history.raw_same") : state === "failed" ? t("history.not_rewritten") : state === "off" ? t("history.rewrite_off") : "";
    }
    if (refined !== "") steps.push({ kind: "mark", name: app ? t("history.flow_put_in", { app }) : t("history.flow_put_in_noapp"), small: "" });
  }
  if (corrected(u)) {
    const final = u.final_text.trim();
    steps.push({ kind: "user", name: t("history.flow_correction"), small: t("history.flow_changes", { n: String(changeCount(refined, final)) }), from: refined, text: final });
  }
  return { steps, unchanged };
}
