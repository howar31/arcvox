import { describe, expect, it } from "vitest";
import type { Profile, ProviderConfig, StepSpec } from "./api";
import { appsSummary, engineMeta, freshName, namesEngine, slugId, templateEngines, withEngines, withLlm, withoutEngine } from "./profileEdit";

const profile = (steps: StepSpec[], llm: string | null = "llm-a"): Profile => ({
  id: "p", name: "P", is_default: true, matcher: { app_ids: [], url_patterns: [] }, asr_provider: null, llm_provider: llm,
  language_hint: null, prompt_addition: "", few_shot: [], preset: null, preset_revision: 0, asr_dictionary_hint: true, llm_dictionary_terms: true, llm_window_title: true, llm_short_fallback: true, steps,
});
const full: StepSpec[] = [{ step: "asr" }, { step: "opencc", config: "s2twp" }, { step: "llm" }, { step: "opencc", config: "s2twp" }, { step: "rules" }];

describe("withEngines", () => {
  it("names the picked engines", () => {
    const next = withEngines(profile(full, "llm-a"), "asr-b", "llm-b");
    expect(next.asr_provider).toBe("asr-b");
    expect(next.llm_provider).toBe("llm-b");
    expect(next.steps).toEqual(full);
  });
  it("follows the default for an empty pick", () => {
    const next = withEngines({ ...profile(full, "llm-a"), asr_provider: "asr-a" }, "", "");
    expect(next.asr_provider).toBeNull();
    expect(next.llm_provider).toBeNull();
  });
  it("turns the rewrite stage off for a null pick, whatever the template had", () => {
    const next = withEngines(profile(full, "llm-a"), "", null);
    expect(next.steps.find((s) => s.step === "llm")).toEqual({ step: "llm", off: true });
    expect(next.llm_provider).toBe("llm-a");
  });
});

describe("withLlm", () => {
  it("turns the llm step off and keeps the engine for when it is enabled again", () => {
    const p = profile(full, "llm-a");
    const next = withLlm(p, null);
    expect(next.llm_provider).toBe("llm-a");
    expect(next.steps).toEqual(full.map((s) => (s.step === "llm" ? { step: "llm", off: true } : s)));
    const back = withLlm(next, "llm-a");
    expect(back.steps).toEqual(full);
  });
  it("restores the llm step after the first opencc step", () => {
    const p = profile([{ step: "asr" }, { step: "opencc", config: "s2twp" }, { step: "opencc", config: "s2twp" }, { step: "rules" }], null);
    const next = withLlm(p, "llm-b");
    expect(next.llm_provider).toBe("llm-b");
    expect(next.steps.map((s) => s.step)).toEqual(["asr", "opencc", "llm", "opencc", "rules"]);
  });
  it("restores the llm step after asr when there is no opencc step", () => {
    const next = withLlm(profile([{ step: "asr" }, { step: "rules" }], null), "llm-b");
    expect(next.steps.map((s) => s.step)).toEqual(["asr", "llm", "rules"]);
  });
  it("keeps the steps when an llm step exists", () => {
    const next = withLlm(profile(full), "llm-b");
    expect(next.llm_provider).toBe("llm-b");
    expect(next.steps).toEqual(full);
  });
  it("rewrites with the default engine for the follow value", () => {
    const next = withLlm(profile([{ step: "asr" }], "llm-a"), "");
    expect(next.llm_provider).toBeNull();
    expect(next.steps.map((s) => s.step)).toEqual(["asr", "llm"]);
  });
  it("does not change its input", () => {
    const p = profile([{ step: "asr" }, { step: "rules" }], null);
    const before = JSON.parse(JSON.stringify(p));
    withLlm(p, "llm-b");
    withLlm(profile(full), null);
    expect(p).toEqual(before);
  });
});

describe("appsSummary", () => {
  it("shows every name up to the limit", () => {
    expect(appsSummary(["Mail", "Outlook"])).toEqual({ shown: ["Mail", "Outlook"], more: 0 });
  });
  it("shows three names and counts the rest", () => {
    expect(appsSummary(["a", "b", "c", "d", "e", "f"])).toEqual({ shown: ["a", "b", "c"], more: 3 });
  });
});

describe("engineMeta", () => {
  const caps = { transcribe: true, chat: false, streaming: false, language_hint: false, hotwords: false };
  const base: ProviderConfig = {
    id: "x", label: "X", kind: "embedded", capabilities: caps, base_url: null, model: null, api_key_ref: null,
    extra_headers: {}, model_path: null, native_id: null, model_id: "m1", runtime: "whisper",
  };
  it("reports an embedded model as installed or missing", () => {
    expect(engineMeta(base, new Set(["m1"]))).toEqual({ kind: "local", ready: true, detail: null });
    expect(engineMeta(base, new Set())).toEqual({ kind: "local", ready: false, detail: null });
  });
  it("checks an imported file against the installed paths", () => {
    const file = { ...base, model_id: null, model_path: "/models/custom/a.bin" };
    expect(engineMeta(file, new Set(), new Set(["/models/custom/a.bin"])).ready).toBe(true);
    expect(engineMeta(file, new Set(), new Set(["/models/custom/b.bin"])).ready).toBe(false);
    // Without the installed paths a set path counts as ready.
    expect(engineMeta(file, new Set()).ready).toBe(true);
  });
  it("reports a native engine as built in", () => {
    expect(engineMeta({ ...base, kind: "native", model_id: null, native_id: "apple.dictation" }, new Set())).toEqual({ kind: "builtin", ready: true, detail: null });
  });
  it("names the host of an http engine", () => {
    const http = { ...base, kind: "http" as const, model_id: null, base_url: "http://localhost:11434/v1" };
    expect(engineMeta(http, new Set())).toEqual({ kind: "external", ready: true, detail: "localhost:11434" });
  });
});

describe("slugId", () => {
  it("makes an ASCII slug from the name", () => {
    expect(slugId("Formal Mail!", [])).toBe("formal-mail");
    expect(slugId("  Code comments (EN) ", [])).toBe("code-comments-en");
  });
  it("falls back to a numbered id for a name without ASCII letters or digits", () => {
    expect(slugId("郵件正式", [])).toBe("profile-1");
    expect(slugId("郵件正式", ["profile-1", "profile-2"])).toBe("profile-3");
  });
  it("adds a suffix when the id is taken", () => {
    expect(slugId("Mail", ["mail"])).toBe("mail-2");
    expect(slugId("Mail", ["mail", "mail-2"])).toBe("mail-3");
  });
  it("never returns the reserved id new", () => {
    expect(slugId("New", [])).toBe("new-2");
    expect(slugId("  new!", ["new-2"])).toBe("new-3");
  });
});

describe("taking an engine off a profile", () => {
  const translate: StepSpec = { step: "translate", target: "English", provider: "x" };
  const p = { ...profile([...full, translate]), asr_provider: "x", llm_provider: "keep", preset: "casual" };
  const presets = [{ id: "casual", app_ids: [], suggested_asr: "breeze", suggested_llm: "x", revision: 1 }];

  it("finds it on any stage", () => {
    expect(namesEngine(p, "x")).toBe(true);
    expect(namesEngine({ ...p, asr_provider: null, steps: full }, "x")).toBe(false);
    expect(namesEngine({ ...p, asr_provider: null, steps: [translate] }, "x")).toBe(true);
  });
  it("moves only the stages on it", () => {
    const moved = withoutEngine(p, "x", { asr: null, llm: null });
    expect([moved.asr_provider, moved.llm_provider]).toEqual([null, "keep"]);
    expect(moved.steps.at(-1)).toEqual({ ...translate, provider: null });
    expect(withoutEngine(p, "x", { asr: "breeze", llm: null }).asr_provider).toBe("breeze");
  });
  it("takes the template's engines, never the one going away", () => {
    expect(templateEngines(p, "x", presets)).toEqual({ asr: "breeze", llm: null });
    expect(templateEngines({ ...p, preset: null }, "x", presets)).toEqual({ asr: null, llm: null });
  });
});

describe("freshName", () => {
  it("keeps a free name and numbers a taken one", () => {
    expect(freshName("休閒", ["Default"])).toBe("休閒");
    expect(freshName("休閒", ["休閒"])).toBe("休閒 2");
    expect(freshName("休閒", ["休閒", "休閒 2"])).toBe("休閒 3");
    expect(freshName(" 正式 ", ["正式"])).toBe("正式 2");
    expect(freshName("Gemini_x", ["gemini_X"], "_")).toBe("Gemini_x_2");
  });
});
