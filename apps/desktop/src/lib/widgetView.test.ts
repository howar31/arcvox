import { beforeEach, describe, expect, it } from "vitest";
import type { MessagePayload } from "./api";
import { keysOf, setLanguage, t } from "./i18n";
import { LABEL_KEYS, labelKey, panelView } from "./widgetView";

const message = (over: Partial<MessagePayload>): MessagePayload => ({ kind: "error", key: "widget.asr_failed", detail: null, copyable: null, code: null, retry: false, language: "zh-TW", ...over });

beforeEach(() => setLanguage("zh-TW"));

describe("widget label", () => {
  it("names the state while no message is up", () => {
    expect(labelKey("recording", null)).toBe("widget.listening");
    expect(labelKey("locked", null)).toBe("widget.locked_short");
    expect(labelKey("idle", null)).toBeNull();
  });
  it("names the kind of message", () => {
    expect(labelKey("idle", message({ kind: "info", key: "widget.cancelled" }))).toBe("widget.cancelled");
    expect(labelKey("idle", message({ key: "widget.no_speech", detail: "no speech" }))).toBe("widget.no_speech_short");
    expect(labelKey("idle", message({ key: "widget.degraded" }))).toBe("widget.rewrite_failed");
    expect(labelKey("idle", message({ kind: "info", key: "widget.secure_input", copyable: "文字" }))).toBe("widget.not_inserted");
    expect(labelKey("idle", message({ copyable: "x", code: "provider_timeout" }))).toBe("widget.failed");
  });
  it("has a text in both languages for every label", () => {
    for (const lang of ["zh-TW", "en"] as const) for (const key of LABEL_KEYS) expect(keysOf(lang), key).toContain(key);
  });
});

describe("panel", () => {
  it("is absent for a state alone and for a notice", () => {
    expect(panelView("recording", null, "", false)).toBeNull();
    expect(panelView("processing", null, "", false)).toBeNull();
    expect(panelView("idle", message({ kind: "info", key: "widget.cancelled" }), "", false)).toBeNull();
    expect(panelView("idle", message({ key: "widget.no_speech", detail: "no speech" }), "", false)).toBeNull();
  });
  it("shows the recognised text while processing and the hint when a hands-free dictation begins", () => {
    expect(panelView("processing", null, "原文", false)).toEqual({ caption: "辨識原文", text: "原文", actions: [], close: false });
    expect(panelView("inserting", null, "原文", false)).toBeNull();
    expect(panelView("locked", null, "", true)?.text).toBe(t("widget.locked"));
    expect(panelView("locked", null, "", false)).toBeNull();
  });
  it("explains a failure and offers the retry when the recording was kept", () => {
    const failed = panelView("idle", message({ copyable: "Speaches: timed out", code: "provider_timeout" }), "", false);
    expect(failed).toEqual({ title: "語音辨識失敗，未插入文字", body: "引擎沒有在時限內回應。", detail: "Speaches: timed out", actions: ["copy_detail"], close: true });
    const kept = panelView("idle", message({ copyable: "x", code: "provider_timeout", retry: true }), "", false);
    expect(kept?.body).toBe("引擎沒有在時限內回應。\n錄音已保留，可到歷史紀錄重試。");
    expect(kept?.actions).toEqual(["copy_detail", "open_history"]);
  });
  it("sends model and engine problems to the engines page without repeating the title", () => {
    const missing = panelView("idle", message({ key: "widget.model_missing", copyable: "x", code: "model_missing" }), "", false);
    expect(missing?.body).toBe("請到設定的「引擎與模型」下載。");
    expect(missing?.actions).toEqual(["copy_detail", "open_engines"]);
    const engine = panelView("idle", message({ key: "widget.engine_unavailable" }), "", false);
    expect(engine).toEqual({ title: "引擎尚未就緒", body: undefined, detail: undefined, actions: ["open_engines"], close: false });
    const apple = panelView("idle", message({ key: "widget.native_unavailable", copyable: "x", code: "native_unavailable" }), "", false);
    expect(apple?.body).toBeUndefined();
  });
  it("shows the system's words for a microphone error", () => {
    const mic = panelView("idle", message({ key: "widget.microphone_unavailable", detail: "device gone" }), "", false);
    expect(mic).toEqual({ title: "無法使用麥克風", body: t("panel.microphone"), detail: "device gone", actions: [], close: false });
  });
  it("offers the text that was not inserted", () => {
    const shown = panelView("idle", message({ kind: "info", key: "widget.app_changed", copyable: "聽寫結果" }), "", false);
    expect(shown).toEqual({ title: t("widget.app_changed"), result: "聽寫結果", actions: ["copy"], close: true });
  });
  it("shows the reason of a failed rewrite", () => {
    const degraded = panelView("idle", message({ key: "widget.degraded", detail: "Ollama: timed out" }), "", false);
    expect(degraded).toEqual({ title: t("widget.degraded"), detail: "Ollama: timed out", actions: [], close: false });
  });
});
