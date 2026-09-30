import { describe, expect, it } from "vitest";
import type { Config, ManifestView, ProviderConfig } from "./api";
import { defaultProfile, isKnownProvider, modelLabel, profileName, providerLabel, usedBy } from "./labels";

const caps = { transcribe: false, chat: false, streaming: false, language_hint: false, hotwords: false };
const base: ProviderConfig = {
  id: "a", label: "A label", kind: "http", capabilities: { ...caps, transcribe: true },
  base_url: null, model: null, api_key_ref: null, extra_headers: {}, model_path: null, native_id: null,
  model_id: null, runtime: null,
};
const httpProvider: ProviderConfig = { ...base, id: "b", label: "B label", capabilities: { ...caps, chat: true } };
const embedded: ProviderConfig = { ...base, id: "e", label: "Embedded", kind: "embedded", runtime: "whisper" };
const profile = (id: string, name: string, asr: string, llm: string | null, isDefault = false) => ({
  id, name, is_default: isDefault, matcher: { app_ids: [], url_patterns: [] }, asr_provider: asr, llm_provider: llm,
  language_hint: null, prompt_addition: "", few_shot: [], steps: [],
});
const cfg = {
  schema_version: 1, general: {}, dictionary: [],
  providers: [base, httpProvider],
  profiles: [profile("mail", "Mail", "a", null), profile("default", "Default", "a", "b", true)],
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
    expect(isKnownProvider(cfg, "a")).toBe(true);
    expect(isKnownProvider(cfg, "gone")).toBe(false);
  });
  it("names profiles and finds the default", () => {
    expect(profileName(cfg, "mail")).toBe("Mail");
    expect(profileName(cfg, "gone")).toBe("gone");
    expect(defaultProfile(cfg)?.id).toBe("default");
    expect(defaultProfile(null)).toBeNull();
  });
  it("lists profiles using a provider, default first", () => {
    expect(usedBy(cfg, "a")).toEqual([{ name: "Default", isDefault: true }, { name: "Mail", isDefault: false }]);
    expect(usedBy(cfg, "b")).toEqual([{ name: "Default", isDefault: true }]);
    expect(usedBy(cfg, "gone")).toEqual([]);
  });
  it("names the model an embedded provider loads", () => {
    expect(modelLabel(manifest, { ...embedded, model_id: "breeze-asr-25-q5_0" })).toBe("Breeze-ASR-25 (q5_0)");
    expect(modelLabel(manifest, { ...embedded, model_id: null, model_path: "/x/y/custom.gguf" })).toBe("custom.gguf");
    expect(modelLabel(null, { ...embedded, model_id: "unknown-id" })).toBe("unknown-id");
    expect(modelLabel(manifest, httpProvider)).toBe(httpProvider.label);
  });
});
