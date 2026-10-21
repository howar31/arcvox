/** The steps a dictation's text went through, for the History page: the transcript, every stage that changed
 * the text, where it was put, and the user's correction read back from the field. Stages come from the
 * records kept for the row; rows whose stage records are gone show one processing step between the
 * transcript and the text that was put in. */
import type { Stage, Utterance } from "./api";
import { cellInput, cellLine, cellNumber, cellOutput, changedCells, insertLines, MODULE_IDS, stageAt, stageName } from "./flowLast";
import { corrected, rawState } from "./historyView";
import { t } from "./i18n";
import { diff } from "./textDiff";

export type FlowStep =
  /** The recognised text, as the engine wrote it. */
  | { kind: "first"; mark: string; name: string; small: string; text: string }
  /** A stage that changed the text; `from` is what it received. */
  | { kind: "stage"; mark: string; name: string; small: string; from: string; text: string }
  /** A stage that failed; `text` is its reason. */
  | { kind: "failed"; mark: string; name: string; small: string; text: string }
  /** Where the text went: into an application, or the clipboard. */
  | { kind: "mark"; name: string; small: string }
  /** The user's correction: `from` is the text that was put in. */
  | { kind: "user"; name: string; small: string; from: string; text: string };

/** Runs of changed text between two versions, a removal next to an addition counted once. */
export function changeCount(from: string, to: string): number {
  let runs = 0, inRun = false;
  for (const c of diff(from, to)) {
    if (c.kind === "keep") inRun = false;
    else if (!inRun) { inRun = true; runs++; }
  }
  return runs;
}

/** The steps, and a line naming the modules that left the text alone (empty when none did). */
export function textFlow(u: Utterance, stages: Stage[] | null): { steps: FlowStep[]; unchanged: string } {
  const steps: FlowStep[] = [];
  const raw = u.raw_text.trim();
  const refined = u.refined_text.trim();
  const app = u.app_name ?? "";
  const kept = stages !== null && stages.length > 0;
  if (raw !== "") {
    const asr = kept ? stageAt(stages, "asr") : undefined;
    const mark = kept ? String(cellNumber(stages, "asr")) : "2";
    const name = stageName("asr");
    const small = asr ? cellLine("asr", stages) : "";
    // The conversion after recognition shows with the recognised text, as the change it made to it.
    const converted = kept ? cellOutput(stages, "asr") : null;
    if (converted !== null && converted.trim() !== raw) steps.push({ kind: "stage", mark, name, small, from: raw, text: converted.trim() });
    else steps.push({ kind: "first", mark, name, small, text: raw });
  }
  let unchanged = "";
  if (kept) {
    const changed = changedCells(stages);
    for (const id of changed) {
      const s = stageAt(stages, id)!;
      const name = stageName(id);
      const small = cellLine(id, stages);
      const mark = String(cellNumber(stages, id));
      const out = cellOutput(stages, id);
      if (s.text === null || out === null) {
        const reason = typeof s.detail.message === "string" ? s.detail.message : typeof s.detail.error === "string" ? s.detail.error : "";
        steps.push({ kind: "failed", mark, name, small, text: reason });
      } else {
        steps.push({ kind: "stage", mark, name, small, from: cellInput(stages, id) ?? "", text: out });
      }
    }
    const left = MODULE_IDS.filter((id) => stageAt(stages, id) && !changed.includes(id)).map(stageName);
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
    const source = u.correction_source === "read_back" ? t("history.flow_source_read_back") : u.correction_source === "hand" ? t("history.flow_source_hand") : "";
    steps.push({ kind: "user", name: t("history.flow_correction"), small: [t("history.flow_changes", { n: String(changeCount(refined, final)) }), source].filter(Boolean).join(" · "), from: refined, text: final });
  }
  return { steps, unchanged };
}
