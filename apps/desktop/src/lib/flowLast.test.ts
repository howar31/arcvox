import { beforeAll, describe, expect, it } from "vitest";
import type { Profile, Stage, Utterance } from "./api";
import { cellLine, changedCount, changedStages, inputOf, insertLines, promptLine, recordLines, runHeadline, stageAt, stageName, stageOpen, summaryLine } from "./flowLast";
import { setLanguage } from "./i18n";

const profile: Profile = {
  id: "default", name: "Default", is_default: true, matcher: { app_ids: [], url_patterns: [] },
  asr_provider: "a", llm_provider: "l", language_hint: null, prompt_addition: "", few_shot: [], asr_dictionary_hint: true, llm_dictionary_terms: true, llm_window_title: true, llm_short_fallback: true,
  steps: [{ step: "asr" }, { step: "opencc", config: "s2tw" }, { step: "llm" }, { step: "opencc", config: "s2tw" }, { step: "rules" }],
};
const stage = (position: number, kind: Stage["kind"], text: string | null, ms: number | null, detail: Record<string, unknown>): Stage => ({ position, kind, text, ms, detail });
const run: Stage[] = [
  stage(1, "capture", null, null, { duration_ms: 6800 }),
  stage(2, "trim", null, null, { removed_ms: 1000 }),
  stage(3, "asr", "這個版本测试過了沒有問題", 1180, { language: "zh", chars: 12 }),
  stage(4, "opencc", "這個版本測試過了沒有問題", 2, { config: "s2tw", changed_chars: 2 }),
  stage(5, "llm", "這個版本測試過了，沒有問題。", 1400, { in_chars: 12, out_chars: 14, prompt: { base_chars: 412, addition_chars: 0, terms: 12, few_shot: 0, window_title: "Slack" } }),
  stage(6, "opencc", "這個版本測試過了，沒有問題。", 1, { config: "s2tw", changed_chars: 0 }),
  stage(7, "rules", "這個版本測試過了，沒有問題。", 0, { fixes: 0, snippets: 0 }),
  stage(8, "insert", null, null, { method: "paste", outcome: "inserted", app_name: "Slack" }),
  stage(9, "record", null, null, { history: true, recording: true }),
];
const row: Utterance = {
  id: 1, ts: 1_700_000_000, profile_id: "default", app_name: "Slack", raw_text: "", refined_text: "", final_text: "", degraded: false,
  has_audio: true, asr_ms: 1180, llm_ms: 1400, audio_ms: 5800, audio_bytes: 1, error: null, error_code: null, redone: 0,
};

describe("flowLast", () => {
  beforeAll(() => setLanguage("en"));

  it("finds a stage by its fixed position and the text a stage received", () => {
    expect(stageAt(run, "opencc2")?.position).toBe(6);
    expect(inputOf(run, "llm")).toBe("這個版本測試過了沒有問題");
    expect(inputOf(run, "capture")).toBeNull();
    expect(inputOf(run, "insert")).toBeNull();
  });

  it("skips a failed step when looking for the text the next one received", () => {
    const failed = run.map((s) => (s.position === 5 ? { ...s, text: null, detail: { error: "provider_timeout", message: "timed out" } } : s));
    expect(inputOf(failed, "opencc2")).toBe("這個版本測試過了沒有問題");
    expect(cellLine("llm", failed)).toBe("Failed, text passed on as it was");
  });

  it("writes the third line of every cell from the last run", () => {
    expect(cellLine("capture", run)).toBe("6.8 s recorded");
    expect(cellLine("trim", run)).toBe("1.0 s removed");
    expect(cellLine("asr", run)).toBe("1.2 s, 12 characters");
    expect(cellLine("opencc1", run)).toBe("Converted 2");
    expect(cellLine("llm", run)).toBe("1.4 s, changed 2");
    expect(cellLine("opencc2", run)).toBe("No change");
    expect(cellLine("rules", run)).toBe("No change");
    expect(cellLine("insert", run)).toBe("Pasted into Slack");
    expect(cellLine("record", run)).toBe("Stored");
  });

  it("leaves the cells empty before the first dictation and names stages that did not run", () => {
    expect(cellLine("llm", null)).toBe("");
    const noInsert = run.filter((s) => s.position !== 8 && s.position !== 5);
    expect(cellLine("insert", noInsert)).toBe("Not inserted");
    expect(cellLine("llm", noInsert)).toBe("Did not run");
  });

  it("tells copied and shown results apart and knows when no recording was kept", () => {
    const copied = run.map((s) => (s.position === 8 ? { ...s, detail: { method: "copy", outcome: "copied", app_name: null } } : s));
    expect(cellLine("insert", copied)).toBe("Copied to the clipboard");
    const shown = run.map((s) => (s.position === 8 ? { ...s, detail: { method: "paste", outcome: "secure_input", app_name: "Terminal" } } : s));
    expect(cellLine("insert", shown)).toBe("Shown on the panel");
    const noAudio = run.map((s) => (s.position === 9 ? { ...s, detail: { history: true, recording: false } } : s));
    expect(cellLine("record", noAudio)).toBe("Stored, no recording kept");
  });

  it("dims the stages a profile does not run", () => {
    expect(stageOpen("llm", profile)).toBe(true);
    expect(stageOpen("llm", { ...profile, llm_provider: null })).toBe(false);
    const bare: Profile = { ...profile, steps: [{ step: "asr" }] };
    expect(stageOpen("opencc1", bare)).toBe(false);
    expect(stageOpen("opencc2", bare)).toBe(false);
    expect(stageOpen("rules", bare)).toBe(false);
    expect(stageOpen("capture", bare)).toBe(true);
    expect(stageOpen("opencc2", { ...profile, steps: [{ step: "asr" }, { step: "opencc", config: "s2tw" }, { step: "llm" }] })).toBe(false);
    const onlySecond: Profile = { ...profile, steps: [{ step: "asr" }, { step: "llm" }, { step: "opencc", config: "s2tw" }] };
    expect(stageOpen("opencc1", onlySecond)).toBe(false);
    expect(stageOpen("opencc2", onlySecond)).toBe(true);
  });

  it("counts a run of removed or added units as one change", () => {
    expect(changedCount("今天 開會", "今天開會。")).toBe(2);
    expect(changedCount("same", "same")).toBe(0);
  });

  it("writes the panel head and the prompt line of the rewrite", () => {
    expect(summaryLine("llm", run)).toBe("1.4 s · 12 characters sent, 14 received");
    expect(summaryLine("asr", run)).toBe("1.2 s, 12 characters");
    expect(promptLine(stageAt(run, "llm")!)).toBe('Prompt: base 412 characters · profile addition 0 characters · 12 dictionary terms · 0 examples · front window "Slack"');
  });

  it("writes the headline with the time, the application and the total of the text stages", () => {
    const now = new Date(1_700_000_000 * 1000 + 3600_000);
    const head = runHeadline(row, run, now);
    expect(head).toMatch(/^Last dictation: \d\d:\d\d, Slack, 2\.6 s in all$/);
    const other = runHeadline({ ...row, app_name: null }, run, new Date(1_700_500_000 * 1000));
    expect(other).toMatch(/^Last dictation: \d+\/\d+ \d\d:\d\d, 2\.6 s in all$/);
  });

  it("lists the stages that changed the text or failed, leaving the rest out", () => {
    expect(changedStages(run)).toEqual(["opencc1", "llm"]);
    // The rewrite failed, so the later stages carried stage 4's text on unchanged.
    const failed = run.map((s) => (s.position === 5 ? { ...s, text: null, detail: { error: "provider_timeout", message: "timed out" } } : s.position > 5 && s.text !== null ? { ...s, text: "這個版本測試過了沒有問題" } : s));
    expect(changedStages(failed)).toEqual(["opencc1", "llm"]);
    expect(changedStages([])).toEqual([]);
    expect(stageName("opencc2")).toBe("Script conversion");
  });

  it("writes the insertion and record panels from the stage details and the row", () => {
    expect(insertLines(run)).toEqual(["Paste at cursor", "put into Slack"]);
    const shown = run.map((s) => (s.position === 8 ? { ...s, detail: { method: "type", outcome: "secure_input", app_name: "Terminal" } } : s));
    expect(insertLines(shown)).toEqual(["Type at cursor", "shown on the panel, the field was a secure input"]);
    expect(insertLines(run.filter((s) => s.position !== 8))).toEqual(["Not inserted"]);
    const size = (n: number) => `${n} B`;
    expect(recordLines(run, row, size)).toEqual(["history stored and counted", "recording 5.8 s, 1 B"]);
    expect(recordLines(run, { ...row, has_audio: false, audio_ms: null, audio_bytes: null }, size)[1]).toBe("the recording's file is gone");
    const noAudio = run.map((s) => (s.position === 9 ? { ...s, detail: { history: true, recording: false } } : s));
    expect(recordLines(noAudio, row, size)[1]).toBe("no recording kept");
  });
});
