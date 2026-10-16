import { describe, expect, it } from "vitest";
import type { Config, InstalledModel, ManifestView, ModelEntry, ProviderConfig } from "./api";
import { blankHttp, encoderMissing, entryBytes, freshId, hasRole, enginesOf, modelServes, recommendedMissing, runnable } from "./engines";
import { ASR_CAPS, blankFor, LLM_CAPS } from "./providerForm";

const provider = (id: string, extra: Partial<ProviderConfig>): ProviderConfig => ({ ...blankFor("embedded"), id, label: id, ...extra });
const file = (id: string, runtime: "whisper" | "llama", custom = false): InstalledModel => ({
  id, runtime, path: `/models/${id}${custom ? "" : "/model.bin"}`, size_bytes: 1, custom,
  capabilities: runtime === "whisper" ? { ...ASR_CAPS } : { ...LLM_CAPS }, encoder: null,
});
const entry = (id: string, role: "asr" | "llm", recommended: boolean): ModelEntry => ({
  id, runtime: role === "asr" ? "whisper" : "llama", role, label: id, description: "", languages: [], url: "", sha256: "",
  size_bytes: 1, license: "", recommended, min_ram_gb: 8,
});

describe("enginesOf", () => {
  it("filters providers by the role's capability", () => {
    const config = { providers: [
      provider("asr", { capabilities: { ...ASR_CAPS } }),
      provider("llm", { kind: "http", capabilities: { ...LLM_CAPS } }),
      provider("both", { capabilities: { ...ASR_CAPS, chat: true } }),
    ] } as Config;
    expect(enginesOf(config, "asr").map((p) => p.id)).toEqual(["asr", "both"]);
    expect(enginesOf(config, "llm").map((p) => p.id)).toEqual(["llm", "both"]);
    expect(enginesOf(null, "asr")).toEqual([]);
  });
  it("keeps an engine with no role reachable on the speech recognition tab", () => {
    const none = provider("none", { capabilities: { ...ASR_CAPS, transcribe: false } });
    const config = { providers: [none] } as Config;
    expect(enginesOf(config, "asr").map((p) => p.id)).toEqual(["none"]);
    expect(enginesOf(config, "llm")).toEqual([]);
    expect(hasRole(none)).toBe(false);
    expect(hasRole(provider("a", { capabilities: { ...ASR_CAPS } }))).toBe(true);
  });
});

describe("recommendedMissing", () => {
  it("returns recommended entries that are not installed", () => {
    const manifest: ManifestView = { source: "bundled", models: [entry("asr-r", "asr", true), entry("asr-x", "asr", false), entry("llm-r", "llm", true)] };
    expect(recommendedMissing(manifest, [file("asr-r", "whisper")]).map((m) => m.id)).toEqual(["llm-r"]);
    expect(recommendedMissing(manifest, [file("asr-r", "whisper"), file("llm-r", "llama")])).toEqual([]);
    expect(recommendedMissing(null, [])).toEqual([]);
  });
});

describe("blankHttp", () => {
  it("is a network service with the capabilities of the role", () => {
    expect(blankHttp("asr")).toMatchObject({ kind: "http", capabilities: ASR_CAPS });
    expect(blankHttp("llm").capabilities).toEqual(LLM_CAPS);
  });
});

describe("freshId", () => {
  it("slugs the text and falls back when nothing is left", () => {
    expect(freshId("Ollama gemma3", ["ollama-gemma3"])).toBe("ollama-gemma3-2");
    expect(freshId("本機", [])).toBe("engine");
  });
});

describe("installed models by use", () => {
  const speech = (encoder: string | null): InstalledModel => ({ ...file("custom/asr.gguf", "llama", true), capabilities: { ...ASR_CAPS, language_hint: false }, encoder });
  const both: InstalledModel = { ...file("custom/omni.gguf", "llama", true), capabilities: { ...ASR_CAPS, chat: true }, encoder: "/models/custom/mmproj-omni.gguf" };

  it("lists a model under each role its file serves", () => {
    expect(modelServes(file("w", "whisper"), "asr")).toBe(true);
    expect(modelServes(file("w", "whisper"), "llm")).toBe(false);
    expect(modelServes(file("g", "llama"), "llm")).toBe(true);
    expect(modelServes(speech("/e"), "llm")).toBe(false);
    expect(modelServes(both, "asr") && modelServes(both, "llm")).toBe(true);
  });

  it("a speech model without its encoder cannot run", () => {
    expect(encoderMissing(speech(null))).toBe(true);
    expect(encoderMissing(speech("/models/custom/mmproj-asr.gguf"))).toBe(false);
    expect(encoderMissing(file("w", "whisper"))).toBe(false);
    expect(encoderMissing(file("g", "llama"))).toBe(false);
    expect(runnable([speech(null), both]).map((m) => m.id)).toEqual(["custom/omni.gguf"]);
  });

  it("a catalogue model's size counts its encoder", () => {
    expect(entryBytes(entry("e", "asr", false))).toBe(1);
    expect(entryBytes({ ...entry("q", "asr", false), runtime: "llama", mmproj: { url: "", sha256: "", size_bytes: 9 } })).toBe(10);
  });
});

