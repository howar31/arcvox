import { describe, expect, it } from "vitest";
import type { Config, InstalledModel, ManifestView, ProviderConfig } from "./api";
import { setLanguage } from "./i18n";
import { defaultProfile, modelDescription, modelLabel, namedBy, profileName, providerLabel, usedBy } from "./labels";

const caps = { transcribe: false, chat: false, streaming: false, language_hint: false, hotwords: false };
const base: ProviderConfig = {
  id: "a", label: "A label", kind: "http", capabilities: { ...caps, transcribe: true },
  base_url: null, model: null, api_key_ref: null, extra_headers: {}, model_path: null, native_id: null,
  model_id: null, runtime: null,
};
const httpProvider: ProviderConfig = { ...base, id: "b", label: "B label", capabilities: { ...caps, chat: true } };
const spare: ProviderConfig = { ...base, id: "c", label: "C label" };
const profile = (id: string, name: string, asr: string | null, llm: string | null, isDefault = false) => ({
  id, name, is_default: isDefault, matcher: { app_ids: [], url_patterns: [] }, asr_provider: asr, llm_provider: llm,
  language_hint: null, prompt_addition: "", few_shot: [], asr_dictionary_hint: true, llm_dictionary_terms: true, llm_window_title: true, llm_short_fallback: true,
  steps: llm === undefined ? [] : [{ step: "asr" }, { step: "llm" }],
});
const cfg = {
  schema_version: 3, general: {}, dictionary: [],
  providers: [base, httpProvider, spare],
  profiles: [profile("mail", "Mail", "a", null), profile("default", "Default", "a", "b", true), profile("follow", "Follower", null, null)],
  default_engines: { asr: "c", llm: "b" },
} as unknown as Config;
const manifest = {
  source: "bundled",
  models: [{ id: "breeze-asr-25-q5_0", label: "Breeze-ASR-25 (q5_0)" }],
} as unknown as ManifestView;

describe("labels", () => {
  it("labels providers and falls back to the id", () => {
    expect(providerLabel(cfg, "a")).toBe("A label");
    expect(providerLabel(cfg, "gone")).toBe("gone");
    expect(providerLabel(cfg, null)).toBe("");
  });
  it("names profiles and finds the default", () => {
    expect(profileName(cfg, "mail")).toBe("Mail");
    expect(profileName(cfg, "gone")).toBe("gone");
    expect(defaultProfile(cfg)?.id).toBe("default");
    expect(defaultProfile(null)).toBeNull();
  });
  it("lists the profiles running with an engine, named or through the default, default profile first", () => {
    expect(usedBy(cfg, "a")).toEqual([{ name: "Default", isDefault: true }, { name: "Mail", isDefault: false }]);
    expect(usedBy(cfg, "c")).toEqual([{ name: "Follower", isDefault: false }]);
    expect(usedBy(cfg, "b")).toEqual([{ name: "Default", isDefault: true }, { name: "Mail", isDefault: false }, { name: "Follower", isDefault: false }]);
    expect(usedBy(cfg, "gone")).toEqual([]);
    // An engine that cannot run is not in use; its profiles are on the default.
    expect(usedBy(cfg, "a", (p) => p.id !== "a")).toEqual([]);
    expect(usedBy(cfg, "c", (p) => p.id !== "a").map((u) => u.name)).toEqual(["Default", "Mail", "Follower"]);
    // What the profiles name, whatever runs.
    expect(namedBy(cfg, "a").map((u) => u.name)).toEqual(["Default", "Mail"]);
    expect(namedBy(cfg, "c")).toEqual([]);
  });
  it("names an installed model: its catalogue label, else its id, or the file name when imported", () => {
    const m = (id: string, custom = false, path = `/m/${id}.bin`): InstalledModel => ({
      id, runtime: "whisper", path, size_bytes: 1, custom,
      capabilities: { transcribe: true, chat: false, streaming: false, language_hint: true, hotwords: true }, encoder: null, facts: { shape: null, quantization: null, license: null }, update: false,
    });
    expect(modelLabel(manifest, m("breeze-asr-25-q5_0"))).toBe("Breeze-ASR-25 (q5_0)");
    expect(modelLabel(null, m("unknown-id"))).toBe("unknown-id");
    expect(modelLabel(manifest, m("custom/x.gguf", true, "/x/y/custom.gguf"))).toBe("custom.gguf");
  });
  it("describes catalogue models in the interface language and falls back to the catalogue text", () => {
    const entry = { id: "gemma-3-4b-it-q4_k_m", description: "General purpose rewriting model." };
    setLanguage("zh-TW");
    expect(modelDescription(entry)).toBe("通用的文字改寫模型");
    expect(modelDescription({ id: "future-model", description: "Some future model." })).toBe("Some future model.");
    setLanguage("en");
    expect(modelDescription(entry)).toBe("General purpose rewriting model.");
    expect(modelDescription({ id: "future-model", description: "" })).toBe("");
  });
});
