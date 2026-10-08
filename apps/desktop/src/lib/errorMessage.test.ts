import { describe, expect, it } from "vitest";
import { errorMessage } from "./api";
import { setLanguage } from "./i18n";

describe("errorMessage", () => {
  it("words a coded failure in the interface language, with its placeholders", () => {
    setLanguage("zh-TW");
    expect(errorMessage({ message: "model `breeze` is not installed", code: "model_not_installed", params: { model: "breeze" } })).toBe("模型 breeze 尚未下載。");
    expect(errorMessage({ message: "a dictation is being processed", code: "busy", params: {} })).toBe("有一筆聽寫正在處理，請等它完成後再試。");
    setLanguage("en");
    expect(errorMessage({ message: "not enough free disk space", code: "disk_full", params: { needed: "2.1", free: "0.4" } })).toBe("Not enough free disk space: 2.1 GB needed, 0.4 GB free.");
  });
  it("falls back to the backend's reason without a code or without a text for it", () => {
    setLanguage("zh-TW");
    expect(errorMessage({ message: "os_too_old" })).toBe("os_too_old");
    expect(errorMessage({ message: "something odd", code: "no_such_code", params: {} })).toBe("something odd");
    expect(errorMessage("plain string")).toBe("plain string");
    setLanguage("en");
  });
  it("words a dictation that failed again by the failure's own code", () => {
    setLanguage("zh-TW");
    expect(errorMessage({ message: "Ollama: timed out", code: "dictation_failed", params: { code: "provider_timeout" } })).toBe("引擎沒有在時限內回應。");
    expect(errorMessage({ message: "odd", code: "dictation_failed", params: { code: "no_such" } })).toBe("odd");
    setLanguage("en");
  });
});
