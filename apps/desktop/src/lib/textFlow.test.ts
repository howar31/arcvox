import { describe, expect, it } from "vitest";
import type { Stage, Utterance } from "./api";
import { changeCount, textFlow } from "./textFlow";

const row = (raw: string, refined: string, fin = refined, app: string | null = "TextEdit"): Utterance => ({
  id: 1, ts: 0, profile_id: "default", app_name: app, raw_text: raw, refined_text: refined, final_text: fin,
  degraded: false, has_audio: false, asr_ms: 200, llm_ms: 1200, audio_ms: null, audio_bytes: null, error: null, error_code: null, redone: 0, correction_source: null, translated: false,
});
const stage = (position: number, kind: Stage["kind"], text: string | null, detail: Record<string, unknown> = {}): Stage => ({ position, kind, text, ms: null, detail });

describe("changeCount", () => {
  it("counts runs of changed text, a removal next to an addition as one", () => {
    expect(changeCount("測試一下，停歇", "測試一下聽寫")).toBe(1);
    expect(changeCount("我明天會晚一點到，大約十點半。", "我後天會晚一點到，大概十點半。")).toBe(2);
    expect(changeCount("same", "same")).toBe(0);
  });
});

describe("textFlow", () => {
  const stages = [
    stage(3, "asr", "测试一下，停歇。", { chars: 8 }),
    stage(4, "opencc", "測試一下，停歇。", { changed_chars: 2 }),
    stage(5, "llm", "測試一下，停歇。", {}),
    stage(7, "rules", "測試一下，停歇", { stop_removed: true }),
    stage(8, "insert", null, { method: "paste", outcome: "inserted", app_name: "TextEdit" }),
  ];
  it("lists the transcript, the stages that changed the text, the insertion and the correction in order", () => {
    const { steps, unchanged } = textFlow(row("测试一下，停歇。", "測試一下，停歇", "測試一下聽寫"), stages);
    // The conversion after recognition shows as the change recognition's step made; the modules are numbered after it.
    expect(steps.map((s) => s.kind)).toEqual(["stage", "stage", "mark", "user"]);
    expect(steps[0]).toMatchObject({ mark: "2", name: "Speech recognition", from: "测试一下，停歇。", text: "測試一下，停歇。" });
    expect(steps[1]).toMatchObject({ mark: "4", name: "Tidy", from: "測試一下，停歇。", text: "測試一下，停歇" });
    expect(steps[2]).toMatchObject({ kind: "mark", name: "Put into TextEdit" });
    expect(steps[3]).toMatchObject({ kind: "user", from: "測試一下，停歇", text: "測試一下聽寫", small: "Changes: 1" });
    expect(unchanged).toBe("Text rewriting left the text as it was.");
  });
  it("names how the correction was made when that was kept", () => {
    const hand = { ...row("测试一下，停歇。", "測試一下，停歇", "測試一下聽寫"), correction_source: "hand" as const };
    expect(textFlow(hand, stages).steps[3]).toMatchObject({ kind: "user", small: "Changes: 1 · by hand" });
    const read = { ...hand, correction_source: "read_back" as const };
    expect(textFlow(read, stages).steps[3]).toMatchObject({ small: "Changes: 1 · read back from the field" });
  });
  it("stops at the insertion when the user changed nothing", () => {
    const { steps } = textFlow(row("测试一下，停歇。", "測試一下，停歇"), stages);
    expect(steps.map((s) => s.kind)).toEqual(["stage", "stage", "mark"]);
    // Without a conversion the recognised text is shown whole, numbered 2 after the capture.
    const plain = textFlow(row("测试一下，停歇。", "測試一下，停歇"), stages.filter((s) => s.kind !== "opencc"));
    expect(plain.steps[0]).toMatchObject({ kind: "first", mark: "2" });
  });
  it("shows one processing step between the transcript and the text put in when the stage records are gone", () => {
    const { steps, unchanged } = textFlow(row("跟他說我明天會晚到", "我明天會晚一點到。", "我明天會晚一點到", "Mail"), []);
    expect(steps.map((s) => s.kind)).toEqual(["first", "stage", "mark", "user"]);
    expect(steps[1]).toMatchObject({ mark: "…", from: "跟他說我明天會晚到", text: "我明天會晚一點到。" });
    expect(steps[2]).toMatchObject({ name: "Put into Mail" });
    expect(unchanged).toBe("");
  });
  it("says so when nothing changed the transcript and no stage records remain", () => {
    const { steps, unchanged } = textFlow(row("好 我等一下看", "好 我等一下看"), null);
    expect(steps.map((s) => s.kind)).toEqual(["first", "mark"]);
    expect(unchanged).toBe("The rewrite did not change the text");
  });
  it("shows a translation whole after the tidy step, numbered after it", () => {
    const translated = [
      stage(1, "asr", "測試一下", { chars: 4 }),
      stage(2, "llm", "測試一下。", {}),
      stage(3, "rules", "測試一下", { stop_removed: true }),
      stage(4, "translate", "A quick test", { target: "英文", in_chars: 4, out_chars: 12 }),
      stage(5, "insert", null, { method: "paste", outcome: "inserted", app_name: "Slack" }),
    ];
    const { steps } = textFlow({ ...row("測試一下", "A quick test", "A quick test", "Slack"), translated: true }, translated);
    expect(steps.map((s) => s.kind)).toEqual(["first", "stage", "stage", "stage", "mark"]);
    expect(steps[3]).toMatchObject({ mark: "5", name: "Translation", small: "Into 英文", from: "測試一下", text: "A quick test" });
  });
  it("carries a failed stage with its reason", () => {
    const failed = [stage(3, "asr", "原文", {}), stage(5, "llm", null, { message: "timed out" }), stage(8, "insert", null, { method: "paste", outcome: "inserted", app_name: "Notes" })];
    const { steps } = textFlow({ ...row("原文", "原文", "原文", "Notes"), degraded: true, llm_ms: null }, failed);
    expect(steps[1]).toMatchObject({ kind: "failed", text: "timed out" });
  });
});
