import { describe, expect, it } from "vitest";
import type { ProviderConfig } from "./api";
import { setLanguage } from "./i18n";
import { blankFor } from "./providerForm";
import { engineKind } from "./stages";

const own = (path: string): ProviderConfig => ({ ...blankFor("embedded"), id: `custom/${path}`, label: path, runtime: "llama", model_path: `/models/custom/${path}` });

describe("engineKind", () => {
  it("names a missing audio encoder instead of a missing download", () => {
    setLanguage("zh-TW");
    const installed = { ids: new Set<string>(), paths: new Set<string>(), deaf: new Set(["/models/custom/asr.gguf"]) };
    expect(engineKind(own("asr.gguf"), installed)).toEqual({ meta: "本機模型，缺少音訊編碼器", missing: true });
    expect(engineKind(own("other.gguf"), installed)).toEqual({ meta: "本機模型，尚未下載", missing: true });
  });
});
