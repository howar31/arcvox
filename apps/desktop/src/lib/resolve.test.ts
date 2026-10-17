import { describe, expect, it } from "vitest";
import type { Config, Profile, ProviderConfig } from "./api";
import { anyFallback, FOLLOW, hasLlmStep, resolveEngines, usableWith } from "./resolve";

const caps = { transcribe: false, chat: false, streaming: false, language_hint: false, hotwords: false };
const engine = (id: string, extra: Partial<ProviderConfig> = {}): ProviderConfig => ({
  id, label: id, kind: "http", capabilities: { ...caps, transcribe: true, chat: true },
  base_url: null, model: null, api_key_ref: null, extra_headers: {}, model_path: null, native_id: null, model_id: null, runtime: null,
  ...extra,
});
const profile = (asr: string | null, llm: string | null, rewrites = true): Profile => ({
  id: "p", name: "P", is_default: false, matcher: { app_ids: [], url_patterns: [] }, asr_provider: asr, llm_provider: llm,
  language_hint: null, prompt_addition: "", few_shot: [], preset: null, preset_revision: 0, asr_dictionary_hint: true, llm_dictionary_terms: true,
  llm_window_title: true, llm_short_fallback: true, steps: rewrites ? [{ step: "asr" }, { step: "llm" }] : [{ step: "asr" }],
});
const cfg = {
  schema_version: 3, general: {}, dictionary: [],
  providers: [engine("d-asr"), engine("d-llm"), engine("far"), engine("local", { kind: "embedded", model_id: "m", runtime: "whisper" })],
  profiles: [],
  default_engines: { asr: "d-asr", llm: "d-llm" },
} as unknown as Config;
const all = () => true;

describe("resolveEngines", () => {
  it("follows the default when the profile names nothing", () => {
    const r = resolveEngines(cfg, profile(null, null), all);
    expect(r.asr).toEqual({ id: "d-asr", source: { kind: "default" } });
    expect(r.llm).toEqual({ id: "d-llm", source: { kind: "default" } });
    expect(anyFallback(r)).toBe(false);
  });
  it("uses the named engine when it is there and can run", () => {
    const r = resolveEngines(cfg, profile("far", "far"), all);
    expect(r.asr).toEqual({ id: "far", source: { kind: "explicit" } });
    expect(r.llm?.source).toEqual({ kind: "explicit" });
  });
  it("falls back to the default, keeping the name, when the engine is gone or cannot run", () => {
    const gone = resolveEngines(cfg, profile("nowhere", null), all);
    expect(gone.asr).toEqual({ id: "d-asr", source: { kind: "fallback", wanted: "nowhere" } });
    const unusable = resolveEngines(cfg, profile("far", "far"), (p) => p.id !== "far");
    expect(unusable.asr.id).toBe("d-asr");
    expect(unusable.llm).toEqual({ id: "d-llm", source: { kind: "fallback", wanted: "far" } });
    expect(anyFallback(unusable)).toBe(true);
  });
  it("has no rewrite engine without a rewrite step", () => {
    const r = resolveEngines(cfg, profile("far", "far", false), all);
    expect(r.llm).toBeNull();
    expect(hasLlmStep(profile(null, null, false))).toBe(false);
  });
  it("reads a local engine's readiness from the installed files, and trusts the rest", () => {
    const local = cfg.providers[3];
    expect(usableWith(new Set(["m"]))(local)).toBe(true);
    expect(usableWith(new Set())(local)).toBe(false);
    expect(usableWith(null)(local)).toBe(true);
    expect(usableWith(new Set())(cfg.providers[2])).toBe(true);
  });
  it("names the follow value the picker uses", () => {
    expect(FOLLOW).toBe("");
  });
});
