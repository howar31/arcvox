import { describe, expect, it } from "vitest";
import type { ProviderConfig } from "./api";
import { setLanguage } from "./i18n";
import { blankFor } from "./providerForm";
import { engineDefaultName, engineDescription, engineGroups, engineKind, engineVariant } from "./stages";

const own = (path: string): ProviderConfig => ({ ...blankFor("embedded"), id: `custom/${path}`, label: path, runtime: "llama", model_path: `/models/custom/${path}` });

describe("engineKind", () => {
  it("names a missing audio encoder instead of a missing download", () => {
    setLanguage("zh-TW");
    const installed = { ids: new Set<string>(), paths: new Set<string>(), deaf: new Set(["/models/custom/asr.gguf"]) };
    expect(engineKind(own("asr.gguf"), installed)).toEqual({ meta: "本機模型，缺少音訊編碼器", missing: true, reason: "encoder" });
    expect(engineKind(own("other.gguf"), installed)).toEqual({ meta: "本機模型，尚未下載", missing: true, reason: "download" });
  });
});

describe("engineGroups", () => {
  const native: ProviderConfig = { ...blankFor("native"), id: "apple-dictation", label: "Apple Dictation", native_id: "apple.dictation" };
  const http: ProviderConfig = { ...blankFor("http"), id: "ollama", label: "Ollama", base_url: "http://localhost:11434/v1" };

  it("keeps the page's group order and puts engines that cannot run last in their group", () => {
    const installed = { ids: new Set<string>(), paths: new Set(["/models/custom/b.gguf"]), deaf: new Set(["/models/custom/a.gguf"]) };
    const groups = engineGroups([http, own("a.gguf"), native, own("b.gguf"), own("c.gguf")], installed);
    expect(groups.map((g) => g.kind)).toEqual(["embedded", "native", "http"]);
    expect(groups[0].engines.map((p) => p.label)).toEqual(["b.gguf", "a.gguf", "c.gguf"]);
    expect(groups[1].engines).toEqual([native]);
  });

  it("leaves out a group without engines", () => {
    expect(engineGroups([native], { ids: null, paths: new Set() }).map((g) => g.kind)).toEqual(["native"]);
  });
});

describe("engineDefaultName", () => {
  const plain = { transcribe_mode: null, reasoning_effort: null };
  it("joins the service, the model and what the engine sets", () => {
    expect(engineDefaultName("Gemini", "models/gemini-3.5-flash-lite", plain)).toBe("Gemini_gemini-3.5-flash-lite");
    expect(engineDefaultName("Gemini", "models/t", { transcribe_mode: "smart", reasoning_effort: "medium" })).toBe("Gemini_t_smart_e-med");
    expect(engineDefaultName("Gemini", "m", { transcribe_mode: null, reasoning_effort: "low" })).toBe("Gemini_m_e-low");
    expect(engineDefaultName("LM Studio", "gemma3:4b", plain)).toBe("LM-Studio_gemma3:4b");
  });
  it("keeps fixed words when a part is missing", () => {
    expect(engineDefaultName("", "m", plain)).toBe("m");
    expect(engineDefaultName("x.test", "", plain)).toBe("x.test");
    expect(engineDefaultName("", "", plain)).toBe("external");
  });
});

describe("engineVariant", () => {
  it("names what an engine sets beyond its model", () => {
    setLanguage("zh-TW");
    expect(engineVariant({ transcribe_mode: null, reasoning_effort: null })).toBe("");
    expect(engineVariant({ transcribe_mode: "smart", reasoning_effort: null })).toBe("智慧模式");
    expect(engineVariant({ transcribe_mode: "smart", reasoning_effort: "low" })).toBe("智慧模式、思考程度低");
  });
  it("is part of the description of a network engine", () => {
    setLanguage("zh-TW");
    const p = { ...blankFor("http"), base_url: "https://g.test/v1", model: "m", transcribe_mode: "smart" } as ProviderConfig;
    expect(engineDescription(p, null)).toContain("模型 m，智慧模式");
    expect(engineDescription({ ...p, transcribe_mode: null }, null)).not.toContain("模式");
  });
});
