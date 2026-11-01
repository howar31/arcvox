import { describe, expect, it } from "vitest";
import type { Stage } from "./api";
import { setLanguage } from "./i18n";
import { usageColumns, usageRows, usageSum } from "./usage";

const stage = (position: number, kind: Stage["kind"], detail: Record<string, unknown>): Stage => ({ position, kind, text: "x", ms: 10, detail });

const interaction = {
  initial_input_tokens: 760, total_input_tokens: 381, total_output_tokens: 23, total_thought_tokens: 0, total_cached_tokens: 0, total_tool_use_tokens: 0, total_tokens: 404,
  input_tokens_by_modality: [{ modality: "audio", tokens: 380 }, { modality: "text", tokens: 1 }],
};
const chat = { prompt_tokens: 458, completion_tokens: 21, total_tokens: 479 };

describe("usage", () => {
  it("reads every number of an interaction", () => {
    setLanguage("zh-TW");
    const [row] = usageRows([stage(3, "asr", { usage: interaction })]);
    expect(row).toMatchObject({ input: 381, audio: 380, text: 1, output: 23, thought: 0, cached: 0, tools: 0, first: 760, total: 404 });
    expect(row.name).toBe("語音辨識");
  });

  it("reads the three numbers of a chat answer and leaves the rest empty", () => {
    const [row] = usageRows([stage(4, "llm", { usage: chat })]);
    expect(row).toMatchObject({ input: 458, output: 21, total: 479, audio: null, thought: null, first: null });
  });

  it("reads the input and output names of a transcription answer", () => {
    const [row] = usageRows([stage(3, "asr", { usage: { input_tokens: 50, output_tokens: 7, total_tokens: 57 } })]);
    expect(row).toMatchObject({ input: 50, output: 7, total: 57 });
  });

  it("lists the stages that reported any, in order, and sums them", () => {
    const rows = usageRows([stage(5, "llm", { usage: chat }), stage(3, "asr", { usage: interaction }), stage(1, "capture", {})]);
    expect(rows.map((r) => r.id)).toEqual(["asr", "llm"]);
    expect(usageSum(rows)).toBe(883);
  });

  it("sums the input and output of a stage that gives no total", () => {
    expect(usageSum(usageRows([stage(4, "llm", { usage: { prompt_tokens: 3, completion_tokens: 4 } })]))).toBe(7);
  });

  it("offers the columns some stage has a number for", () => {
    const rows = usageRows([stage(4, "llm", { usage: chat })]);
    expect(usageColumns(rows)).toEqual(["total", "input", "output"]);
    expect(usageColumns(usageRows([stage(3, "asr", { usage: interaction })]))).toEqual(["total", "input", "audio", "text", "output", "thought", "cached", "tools", "first"]);
  });

  it("has no rows without usage", () => {
    expect(usageRows([stage(3, "asr", { chars: 3 })])).toEqual([]);
    expect(usageRows(null)).toEqual([]);
  });
});
