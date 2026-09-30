import { describe, expect, it } from "vitest";
import type { Config, InstalledModel, ManifestView, ModelEntry, ProviderConfig } from "./api";
import { blankForRole, engineFromModel, freshId, hasRole, enginesOf, recommendedMissing, unusedModels } from "./engines";
import { ASR_CAPS, blankFor, LLM_CAPS, NATIVE_DICTATION, NATIVE_FM } from "./providerForm";

const provider = (id: string, extra: Partial<ProviderConfig>): ProviderConfig => ({ ...blankFor("embedded"), id, label: id, ...extra });
const file = (id: string, runtime: "whisper" | "llama", custom = false): InstalledModel => ({
  id, runtime, path: `/models/${id}${custom ? "" : "/model.bin"}`, size_bytes: 1, custom,
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

describe("unusedModels", () => {
  it("lists files of the role's runtime that no embedded provider loads", () => {
    const installed = [file("a", "whisper"), file("b", "whisper"), file("custom/c.bin", "whisper", true), file("g", "llama")];
    const providers = [
      provider("pa", { model_id: "a" }),
      provider("pc", { model_path: "/models/custom/c.bin" }),
      // An external service that happens to name the model does not load the file.
      provider("http", { kind: "http", model_id: "b" }),
    ];
    expect(unusedModels(installed, providers, "asr").map((m) => m.id)).toEqual(["b"]);
    expect(unusedModels(installed, providers, "llm").map((m) => m.id)).toEqual(["g"]);
  });
});

describe("engineFromModel", () => {
  it("builds an embedded engine for a catalogue model", () => {
    const p = engineFromModel(file("gemma-3-4b-it-q4_k_m", "llama"), "Gemma 3 4B", []);
    expect(p).toMatchObject({ id: "gemma-3-4b-it-q4-k-m", label: "Gemma 3 4B", kind: "embedded", runtime: "llama", model_id: "gemma-3-4b-it-q4_k_m", model_path: null });
    expect(p.capabilities).toEqual(LLM_CAPS);
  });
  it("loads an imported file by path and suffixes a taken id", () => {
    const m = file("custom/My Model.bin", "whisper", true);
    const p = engineFromModel(m, "My Model.bin", ["my-model", "my-model-2"]);
    expect(p.id).toBe("my-model-3");
    expect(p.model_path).toBe(m.path);
    expect(p.model_id).toBeNull();
    expect(p.capabilities).toEqual(ASR_CAPS);
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

describe("blankForRole", () => {
  it("presets the runtime, the system service and the capabilities to the role", () => {
    expect(blankForRole("embedded", "llm")).toMatchObject({ kind: "embedded", runtime: "llama", capabilities: LLM_CAPS });
    expect(blankForRole("embedded", "asr")).toMatchObject({ runtime: "whisper", capabilities: ASR_CAPS });
    expect(blankForRole("native", "llm")).toMatchObject({ native_id: NATIVE_FM, capabilities: LLM_CAPS });
    expect(blankForRole("native", "asr")).toMatchObject({ native_id: NATIVE_DICTATION, capabilities: ASR_CAPS });
    expect(blankForRole("http", "asr")).toMatchObject({ kind: "http", capabilities: ASR_CAPS });
    expect(blankForRole("http", "llm").capabilities).toEqual(LLM_CAPS);
  });
});

describe("freshId", () => {
  it("slugs the text and falls back when nothing is left", () => {
    expect(freshId("Ollama gemma3", ["ollama-gemma3"])).toBe("ollama-gemma3-2");
    expect(freshId("本機", [])).toBe("engine");
  });
});
