import { describe, expect, it } from "vitest";
import type { Profile, ProviderConfig, StepSpec } from "./api";
import { appsSummary, blankProfile, engineMeta, slugId, withLlm } from "./profileEdit";

const profile = (steps: StepSpec[], llm: string | null = "llm-a"): Profile => ({
  id: "p", name: "P", is_default: true, matcher: { app_ids: [], url_patterns: [] }, asr_provider: "asr-a", llm_provider: llm,
  language_hint: null, prompt_addition: "", few_shot: [], steps,
});
const full: StepSpec[] = [{ step: "asr" }, { step: "opencc", config: "s2twp" }, { step: "llm" }, { step: "opencc", config: "s2twp" }, { step: "rules" }];

describe("withLlm", () => {
  it("removes every llm step and clears the engine", () => {
    const p = profile([...full, { step: "llm" }]);
    const next = withLlm(p, null);
    expect(next.llm_provider).toBeNull();
    expect(next.steps.map((s) => s.step)).toEqual(["asr", "opencc", "opencc", "rules"]);
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

describe("blankProfile", () => {
  it("copies the engines, language and steps of the base and nothing else", () => {
    const base: Profile = { ...profile(full, "llm-a"), id: "default", name: "Default", language_hint: "ja", prompt_addition: "x",
      few_shot: [{ raw: "a", refined: "b" }], matcher: { app_ids: ["com.example.a"], url_patterns: ["example.com"] } };
    const blank = blankProfile(base);
    expect(blank).toEqual({
      id: "", name: "", is_default: false, matcher: { app_ids: [], url_patterns: [] }, asr_provider: "asr-a", llm_provider: "llm-a",
      language_hint: "ja", prompt_addition: "", few_shot: [], steps: full,
    });
    blank.steps.push({ step: "rules" });
    expect(base.steps).toEqual(full);
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
