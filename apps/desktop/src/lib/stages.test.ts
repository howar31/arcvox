import { describe, expect, it } from "vitest";
import type { ProviderConfig } from "./api";
import { setLanguage } from "./i18n";
import { blankFor } from "./providerForm";
import { engineGroups, engineKind } from "./stages";

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
