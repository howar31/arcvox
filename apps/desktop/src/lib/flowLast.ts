// What the Profiles page says about the last dictation's stages: the third line of each
// cell, the head line of the "last run" panel, and which stages a profile runs at all.

import type { Profile, Stage, Utterance } from "./api";
import { formatShortDate, formatTime } from "./format";
import { t } from "./i18n";
import { scriptAt } from "./flowEdit";
import { diff } from "./textDiff";

/** The nine stages in order; the two script conversions are told apart by their place. */
export type StageId = "capture" | "trim" | "asr" | "opencc1" | "llm" | "opencc2" | "rules" | "insert" | "record";
export const STAGE_IDS: StageId[] = ["capture", "trim", "asr", "opencc1", "llm", "opencc2", "rules", "insert", "record"];

const POSITION: Record<StageId, number> = { capture: 1, trim: 2, asr: 3, opencc1: 4, llm: 5, opencc2: 6, rules: 7, insert: 8, record: 9 };

/** The record of a stage in a run, by its fixed position. */
export function stageAt(stages: Stage[], id: StageId): Stage | undefined {
  return stages.find((s) => s.position === POSITION[id]);
}

/** The text a text stage received: the nearest earlier stage that handed text on; null for the audio stages and after the text. */
export function inputOf(stages: Stage[], id: StageId): string | null {
  const pos = POSITION[id];
  if (pos < 4 || pos > 7) return null;
  for (let p = pos - 1; p >= 3; p--) {
    const s = stages.find((x) => x.position === p);
    if (s?.text != null) return s.text;
  }
  return null;
}

/** Whether a profile runs a stage; a closed stage is shown dimmed. */
export function stageOpen(id: StageId, profile: Profile): boolean {
  const kinds = profile.steps.map((s) => s.step);
  switch (id) {
    case "llm": return profile.llm_provider !== null && kinds.includes("llm");
    case "opencc1": return scriptAt(profile, 1) !== null;
    case "opencc2": return scriptAt(profile, 2) !== null;
    case "rules": return kinds.includes("rules");
    default: return true;
  }
}

/** Changes between two texts, counting each run of removed or added units as one. */
export function changedCount(from: string, to: string): number {
  let n = 0, inChange = false;
  for (const c of diff(from, to)) {
    if (c.kind === "keep") inChange = false;
    else if (!inChange) { inChange = true; n++; }
  }
  return n;
}

const secs = (ms: number) => (ms / 1000).toFixed(1);
const num = (v: unknown, fallback = 0): number => (typeof v === "number" ? v : fallback);
const str = (v: unknown): string | null => (typeof v === "string" ? v : null);

/** The third line of a cell: what the stage did in the last run; "" when nothing ran. */
export function cellLine(id: StageId, stages: Stage[] | null): string {
  if (!stages) return "";
  const s = stageAt(stages, id);
  if (!s) return id === "insert" ? t("flow.cell.insert_none") : t("flow.cell.not_run");
  const d = s.detail;
  switch (id) {
    case "capture": return t("flow.cell.recorded", { secs: secs(num(d.duration_ms)) });
    case "trim": {
      const removed = num(d.removed_ms);
      return removed > 0 ? t("flow.cell.removed", { secs: secs(removed) }) : t("flow.cell.removed_none");
    }
    case "asr": return t("flow.cell.asr", { secs: secs(num(s.ms)), chars: String(num(d.chars, s.text?.length ?? 0)) });
    case "opencc1":
    case "opencc2": {
      if (s.text === null) return t("flow.cell.failed");
      const n = num(d.changed_chars);
      return n > 0 ? t("flow.cell.converted", { n: String(n) }) : t("flow.cell.unchanged");
    }
    case "llm": {
      if (s.text === null) return t("flow.cell.failed");
      const from = inputOf(stages, "llm");
      const n = from === null ? 0 : changedCount(from, s.text);
      return n > 0 ? t("flow.cell.llm", { secs: secs(num(s.ms)), n: String(n) }) : t("flow.cell.unchanged");
    }
    case "rules": {
      if (s.text === null) return t("flow.cell.failed");
      const n = num(d.fixes) + num(d.snippets);
      return n > 0 ? t("flow.cell.fixes", { n: String(n) }) : t("flow.cell.unchanged");
    }
    case "insert": {
      const outcome = str(d.outcome), method = str(d.method) ?? "paste", app = str(d.app_name);
      if (outcome === "inserted") {
        const how = method === "type" || method === "ax" ? method : "paste";
        return app ? t(`flow.cell.inserted.${how}`, { app }) : t(`flow.cell.inserted_noapp.${how}`);
      }
      if (outcome === "copied") return t("flow.cell.copied");
      return t("flow.cell.shown");
    }
    case "record": return d.recording === false ? t("flow.cell.recorded_no_audio") : t("flow.cell.recorded_all");
  }
}

/** The head line of the "last run" panel for a stage; the rewrite tells what was sent and received. */
export function summaryLine(id: StageId, stages: Stage[]): string {
  const s = stageAt(stages, id);
  if (id === "llm" && s?.text != null) {
    return t("flow.panel.sent_received", { secs: secs(num(s.ms)), in: String(num(s.detail.in_chars)), out: String(num(s.detail.out_chars)) });
  }
  return cellLine(id, stages);
}

/** The prompt line of the rewrite panel: what the model was given. */
export function promptLine(stage: Stage): string {
  const p = (stage.detail.prompt ?? {}) as Record<string, unknown>;
  const title = str(p.window_title);
  return t("flow.panel.prompt", {
    base: String(num(p.base_chars)), add: String(num(p.addition_chars)), terms: String(num(p.terms)), shots: String(num(p.few_shot)),
    title: title ? t("flow.panel.prompt_title", { title }) : "",
  });
}

/** When the last dictation happened: the time today, else the date and time. */
export function whenLabel(ts: number, now: Date): string {
  const d = new Date(ts * 1000);
  const sameDay = d.getFullYear() === now.getFullYear() && d.getMonth() === now.getMonth() && d.getDate() === now.getDate();
  return sameDay ? formatTime(ts) : `${formatShortDate(ts)} ${formatTime(ts)}`;
}

/** The line above the stages: when the last dictation was, where, and how long its text stages took in all. */
export function runHeadline(row: Utterance, stages: Stage[], now: Date): string {
  const total = stages.filter((s) => s.position >= 3 && s.position <= 7).reduce((n, s) => n + (s.ms ?? 0), 0);
  const vars = { time: whenLabel(row.ts, now), secs: secs(total), app: row.app_name ?? "" };
  return row.app_name ? t("flow.last_head", vars) : t("flow.last_head_noapp", vars);
}

/** The stage's name as the pages show it. */
export function stageName(id: StageId): string {
  switch (id) {
    case "capture": return t("home.stage.capture");
    case "trim": return t("flow.stage.trim");
    case "asr": return t("home.stage.asr");
    case "opencc1":
    case "opencc2": return t("flow.stage.opencc");
    case "llm": return t("home.stage.llm");
    case "rules": return t("flow.stage.rules");
    case "insert": return t("flow.stage.insert");
    case "record": return t("flow.stage.record");
  }
}

/** The text stages after recognition that changed the text or failed, in order; the ones that passed it on unchanged are left out. */
export function changedStages(stages: Stage[]): StageId[] {
  return (["opencc1", "llm", "opencc2", "rules"] as StageId[]).filter((id) => {
    const s = stageAt(stages, id);
    if (!s) return false;
    return s.text === null || s.text !== inputOf(stages, id);
  });
}

/** The parts of the insertion panel's one line: the method used and what became of the text. */
export function insertLines(stages: Stage[]): string[] {
  const s = stageAt(stages, "insert");
  if (!s) return [t("flow.cell.insert_none")];
  const method = str(s.detail.method) ?? "paste";
  const outcome = str(s.detail.outcome) ?? "inserted";
  const known = ["inserted", "copied", "secure_input", "app_changed", "not_trusted", "clipboard_failed", "empty_text"];
  const app = str(s.detail.app_name);
  const what = known.includes(outcome) ? outcome : "inserted";
  return [
    t(`home.output_${["paste", "type", "ax", "copy"].includes(method) ? method : "paste"}`),
    what === "inserted" && !app ? t("flow.panel.insert_outcome.inserted_noapp") : t(`flow.panel.insert_outcome.${what}`, { app: app ?? "" }),
  ];
}

/** The parts of the record panel's one line: what was stored, with the recording's length and size when the row still has it. */
export function recordLines(stages: Stage[], row: Utterance | null, size: (bytes: number) => string): string[] {
  const s = stageAt(stages, "record");
  if (!s) return [t("flow.cell.not_run")];
  const kept = s.detail.recording !== false;
  const audio = !kept ? t("flow.panel.record_audio_none")
    : row?.has_audio && row.audio_ms !== null && row.audio_bytes !== null ? t("flow.panel.record_audio", { secs: secs(row.audio_ms), size: size(row.audio_bytes) })
    : t("flow.panel.record_audio_gone");
  return [t("flow.panel.record_history"), audio];
}

