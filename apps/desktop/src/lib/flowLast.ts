// What the Profiles page says about the last dictation's stages: the third line of each
// cell, the head line of the "last run" panel, and which cells a profile has at all.

import type { Profile, Stage, Utterance } from "./api";
import { formatShortDate, formatTime } from "./format";
import { t } from "./i18n";
import { diff } from "./textDiff";

/** The records a run may hold, in the order they run; the two script conversions are told apart by their place around the rewrite. */
export type StageId = "capture" | "trim" | "asr" | "opencc1" | "llm" | "opencc2" | "rules" | "translate" | "insert" | "record";
export const STAGE_IDS: StageId[] = ["capture", "trim", "asr", "opencc1", "llm", "opencc2", "rules", "translate", "insert", "record"];

/** The records that carry text, in the order they run. */
const TEXT_KINDS = ["asr", "opencc", "llm", "rules", "translate"];

/**
 * The cells of the Profiles page, in their fixed order: capture holds the trim, speech recognition the
 * conversion after it, the rewrite the conversion after itself. The modules are the cells a profile adds
 * or removes; the others every profile has.
 */
export type CellId = "capture" | "asr" | "llm" | "rules" | "translate" | "insert" | "record";
export const CELL_IDS: CellId[] = ["capture", "asr", "llm", "rules", "translate", "insert", "record"];
export const MODULE_IDS: CellId[] = ["llm", "rules", "translate"];

/** Whether a profile has a module: its step is in the profile's steps. */
export function moduleAdded(profile: Profile, id: CellId): boolean {
  return profile.steps.some((s) => s.step === id);
}

/** The cells a profile shows: the base ones and the modules it has added. */
export function cellsOf(profile: Profile): CellId[] {
  return CELL_IDS.filter((id) => !MODULE_IDS.includes(id) || moduleAdded(profile, id));
}

/** The number a cell shows in a run: its place among the base cells and the modules that run holds. */
export function cellNumber(stages: Stage[], id: CellId): number {
  const present = CELL_IDS.filter((c) => !MODULE_IDS.includes(c) || stageIndex(stages, c) >= 0);
  return present.indexOf(id) + 1;
}

/** Where a stage's record sits in a run (the records come in running order); -1 when it did not run.
 * A stage is found by its kind: a script conversion before the rewrite is the first, one after it the second. */
export function stageIndex(stages: Stage[], id: StageId): number {
  if (id === "opencc1" || id === "opencc2") {
    const llm = stages.findIndex((s) => s.kind === "llm");
    return stages.findIndex((s, i) => s.kind === "opencc" && (id === "opencc1" ? llm < 0 || i < llm : llm >= 0 && i > llm));
  }
  return stages.findIndex((s) => s.kind === id);
}

/** The record of a stage in a run, by its kind and place. */
export function stageAt(stages: Stage[], id: StageId): Stage | undefined {
  const i = stageIndex(stages, id);
  return i < 0 ? undefined : stages[i];
}

/** The text a text stage received: the nearest earlier stage that handed text on; null for the audio stages and after the text. */
export function inputOf(stages: Stage[], id: StageId): string | null {
  const at = stageIndex(stages, id);
  if (at < 0 || !TEXT_KINDS.includes(stages[at].kind)) return null;
  for (let i = at - 1; i >= 0; i--) {
    const s = stages[i];
    if (!TEXT_KINDS.includes(s.kind)) continue;
    if (s.text != null) return s.text;
  }
  return null;
}

/** The text a cell received: what its first record got. */
export function cellInput(stages: Stage[], id: CellId): string | null {
  return id === "llm" || id === "rules" || id === "translate" ? inputOf(stages, id) : null;
}

/** The text a cell handed on: after the conversion it holds, when that ran; null when the cell's stage failed or carries no text. */
export function cellOutput(stages: Stage[], id: CellId): string | null {
  if (id === "asr") return stageAt(stages, "opencc1")?.text ?? stageAt(stages, "asr")?.text ?? null;
  if (id === "llm") {
    const llm = stageAt(stages, "llm");
    if (!llm || llm.text === null) return null;
    return stageAt(stages, "opencc2")?.text ?? llm.text;
  }
  if (id === "rules" || id === "translate") return stageAt(stages, id)?.text ?? null;
  return null;
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

/** How many characters the conversion a cell holds changed; 0 when it did not run. */
function converted(stages: Stage[], id: "asr" | "llm"): number {
  const s = stageAt(stages, id === "asr" ? "opencc1" : "opencc2");
  return s && s.text !== null ? num(s.detail.changed_chars) : 0;
}

/** The third line of a cell: what the stage did in the last run; "" when nothing ran. */
export function cellLine(id: CellId, stages: Stage[] | null): string {
  if (!stages) return "";
  const s = stageAt(stages, id);
  if (!s) return id === "insert" ? t("flow.cell.insert_none") : t("flow.cell.not_run");
  const d = s.detail;
  switch (id) {
    case "capture": {
      const removed = num(stageAt(stages, "trim")?.detail.removed_ms);
      const vars = { secs: secs(num(d.duration_ms)), removed: secs(removed) };
      return removed > 0 ? t("flow.cell.capture_trimmed", vars) : t("flow.cell.recorded", vars);
    }
    case "asr": {
      const n = converted(stages, "asr");
      const vars = { chars: String(num(d.chars, s.text?.length ?? 0)), n: String(n) };
      return n > 0 ? t("flow.cell.asr_converted", vars) : t("flow.cell.asr", vars);
    }
    case "llm": {
      if (s.text === null) return t("flow.cell.failed");
      const from = inputOf(stages, "llm");
      const n = from === null ? 0 : changedCount(from, s.text);
      const m = converted(stages, "llm");
      if (m > 0) return t("flow.cell.llm_converted", { n: String(n), m: String(m) });
      return n > 0 ? t("flow.cell.llm", { n: String(n) }) : t("flow.cell.unchanged");
    }
    case "rules": {
      if (s.text === null) return t("flow.cell.failed");
      const parts: string[] = [];
      const fixes = num(d.fixes) + num(d.snippets);
      if (fixes > 0) parts.push(t("flow.cell.fixes", { n: String(fixes) }));
      const widened = num(d.widened);
      if (widened > 0) parts.push(t("flow.cell.widened", { n: String(widened) }));
      const spaces = num(d.spaces);
      if (spaces > 0) parts.push(t("flow.cell.spaces", { n: String(spaces) }));
      if (d.stop_removed === true) parts.push(t("flow.cell.stop_removed"));
      if (d.question_added === true) parts.push(t("flow.cell.question_added"));
      const inside = num(d.questions_inside);
      if (inside > 0) parts.push(t("flow.cell.questions_inside", { n: String(inside) }));
      return parts.length > 0 ? parts.join(t("flow.cell.join")) : t("flow.cell.unchanged");
    }
    case "translate": {
      if (s.text === null) return t("flow.cell.failed");
      return t("flow.cell.translated", { target: str(d.target) ?? "" });
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

/** A cell's time in the last run, as the rail's tag shows it; null for a cell that did not run or has no stored time. */
export function cellTime(id: CellId, stages: Stage[] | null): string | null {
  const s = stages ? stageAt(stages, id) : undefined;
  return s?.ms == null ? null : timeLabel(s.ms);
}

/** A duration: seconds with one decimal from one second up, milliseconds below; a stage that took no whole millisecond is told as under one. */
export function timeLabel(ms: number): string {
  if (ms >= 1000) return t("flow.time_s", { secs: secs(ms) });
  return ms < 1 ? t("flow.time_under_ms") : t("flow.time_ms", { ms: String(Math.round(ms)) });
}

/** The head line of the "last run" panel for a cell; recognition and the rewrite tell their time too. */
export function summaryLine(id: CellId, stages: Stage[]): string {
  const s = stageAt(stages, id);
  if (id === "asr" && s?.text != null) {
    const n = converted(stages, "asr");
    const vars = { secs: secs(num(s.ms)), chars: String(num(s.detail.chars, s.text.length)), n: String(n) };
    return n > 0 ? t("flow.panel.asr_converted", vars) : t("flow.panel.asr", vars);
  }
  if (id === "llm" && s?.text != null) {
    return t("flow.panel.sent_received", { secs: secs(num(s.ms)), in: String(num(s.detail.in_chars)), out: String(num(s.detail.out_chars)) });
  }
  if (id === "translate" && s?.text != null) {
    return t("flow.panel.translated", { secs: secs(num(s.ms)), target: str(s.detail.target) ?? "", in: String(num(s.detail.in_chars)), out: String(num(s.detail.out_chars)) });
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
  const total = stages.filter((s) => TEXT_KINDS.includes(s.kind)).reduce((n, s) => n + (s.ms ?? 0), 0);
  const vars = { time: whenLabel(row.ts, now), secs: secs(total), app: row.app_name ?? "" };
  return row.app_name ? t("flow.last_head", vars) : t("flow.last_head_noapp", vars);
}

/** The cell's name as the pages show it. */
export function stageName(id: CellId): string {
  switch (id) {
    case "capture": return t("home.stage.capture");
    case "asr": return t("home.stage.asr");
    case "llm": return t("home.stage.llm");
    case "rules": return t("flow.stage.rules");
    case "translate": return t("flow.stage.translate");
    case "insert": return t("flow.stage.insert");
    case "record": return t("flow.stage.record");
  }
}

/** The modules after recognition that changed the text or failed, in order; the ones that passed it on unchanged are left out. */
export function changedCells(stages: Stage[]): CellId[] {
  return MODULE_IDS.filter((id) => {
    const s = stageAt(stages, id);
    if (!s) return false;
    return s.text === null || cellOutput(stages, id) !== cellInput(stages, id);
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
