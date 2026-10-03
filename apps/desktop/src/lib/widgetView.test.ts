import { beforeEach, describe, expect, it } from "vitest";
import type { MessagePayload } from "./api";
import { keysOf, setLanguage, t } from "./i18n";
import { LABEL_KEYS, dictationLabelKey, labelKey, panelView, ringView } from "./widgetView";

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
  it("says that the text went in, until something else happens", () => {
    expect(labelKey("idle", null, true)).toBe("widget.inserted");
    expect(labelKey("recording", null, true)).toBe("widget.listening");
    expect(labelKey("idle", message({ key: "widget.degraded" }), true)).toBe("widget.rewrite_failed");
    expect(LABEL_KEYS).toContain("widget.inserted");
  });
  it("has a text in both languages for every label", () => {
    for (const lang of ["zh-TW", "en"] as const) for (const key of LABEL_KEYS) expect(keysOf(lang), key).toContain(key);
  });
});

describe("ring", () => {
  it("is the same colour while listening and while working, and moves differently", () => {
    expect(ringView("recording", null, false)).toEqual({ tone: "accent", motion: "level", glyph: null });
    expect(ringView("locked", null, false)).toEqual({ tone: "accent", motion: "level", glyph: null });
    expect(ringView("processing", null, false)).toEqual({ tone: "accent", motion: "spin", glyph: null });
    expect(ringView("inserting", null, false)).toEqual({ tone: "accent", motion: "close", glyph: null });
  });
  it("is green with a check mark once the text went in", () => {
    expect(ringView("idle", null, true)).toEqual({ tone: "ok", motion: "once", glyph: "check" });
    expect(ringView("idle", null, false)).toEqual({ tone: "quiet", motion: "none", glyph: null });
  });
  it("is grey when nothing came of it or nothing is going on", () => {
    expect(ringView("idle", message({ kind: "info", key: "widget.cancelled" }), false)).toEqual({ tone: "quiet", motion: "settle", glyph: null });
    expect(ringView("idle", message({ key: "widget.no_speech", detail: "no speech" }), false).tone).toBe("quiet");
    expect(ringView("placing", null, false)).toEqual({ tone: "quiet", motion: "drag", glyph: null });
  });
  it("asks for attention when the text exists but is not where or what was wanted", () => {
    const warn = { tone: "warn", motion: "twice", glyph: "bang" };
    expect(ringView("idle", message({ kind: "info", key: "widget.app_changed", copyable: "文字" }), false)).toEqual(warn);
    expect(ringView("idle", message({ key: "widget.degraded" }), false)).toEqual(warn);
  });
  it("is red only for a failure", () => {
    const fail = { tone: "fail", motion: "shake", glyph: "cross" };
    expect(ringView("idle", message({}), false)).toEqual(fail);
    expect(ringView("idle", message({ key: "widget.empty_result" }), false)).toEqual(fail);
    expect(ringView("idle", message({ copyable: "x", code: "provider_timeout" }), true)).toEqual(fail);
  });
});

describe("panel", () => {
  it("is absent for a state alone and for a notice", () => {
    expect(panelView("recording", null, "", null, false)).toBeNull();
    expect(panelView("processing", null, "", null, false)).toBeNull();
    expect(panelView("idle", message({ kind: "info", key: "widget.cancelled" }), "", null, false)).toBeNull();
    expect(panelView("idle", message({ key: "widget.no_speech", detail: "no speech" }), "", null, false)).toBeNull();
  });
  it("shows the recognised text while processing and the hint when a hands-free dictation begins", () => {
    expect(panelView("processing", null, "原文", null, false)).toEqual({ caption: "辨識原文", text: "原文", actions: [], close: false });
    expect(panelView("inserting", null, "原文", null, false)).toBeNull();
    expect(panelView("locked", null, "", null, true)?.text).toBe(t("widget.locked"));
    expect(panelView("locked", null, "", null, false)).toBeNull();
  });
  it("shows the final text taking the place of the recognised text, until the widget goes", () => {
    const view = { caption: "最終文字", rewrite: { from: "原文", to: "整理後" }, actions: [], close: false };
    expect(panelView("processing", null, "原文", "整理後", false)).toEqual(view);
    expect(panelView("inserting", null, "原文", "整理後", false)).toEqual(view);
    expect(panelView("idle", null, "原文", "整理後", false)).toEqual(view);
    expect(panelView("idle", null, "", "整理後", false)).toBeNull();
    // A message takes the panel.
    expect(panelView("idle", message({ key: "widget.degraded", detail: "x" }), "原文", "原文", false)?.rewrite).toBeUndefined();
  });
  it("explains a failure and offers the retry when the recording was kept", () => {
    const failed = panelView("idle", message({ copyable: "Speaches: timed out", code: "provider_timeout" }), "", null, false);
    expect(failed).toEqual({ title: "語音辨識失敗，未插入文字", body: "引擎沒有在時限內回應。", detail: "Speaches: timed out", actions: ["copy_detail"], close: true });
    const kept = panelView("idle", message({ copyable: "x", code: "provider_timeout", retry: true }), "", null, false);
    expect(kept?.body).toBe("引擎沒有在時限內回應。\n錄音已保留，可到歷史紀錄重試。");
    expect(kept?.actions).toEqual(["copy_detail", "open_history"]);
  });
  it("sends model and engine problems to the engines page without repeating the title", () => {
    const missing = panelView("idle", message({ key: "widget.model_missing", copyable: "x", code: "model_missing" }), "", null, false);
    expect(missing?.body).toBe("請到設定的「引擎與模型」下載。");
    expect(missing?.actions).toEqual(["copy_detail", "open_engines"]);
    const engine = panelView("idle", message({ key: "widget.engine_unavailable" }), "", null, false);
    expect(engine).toEqual({ title: "引擎尚未就緒", body: undefined, detail: undefined, actions: ["open_engines"], close: false });
    const apple = panelView("idle", message({ key: "widget.native_unavailable", copyable: "x", code: "native_unavailable" }), "", null, false);
    expect(apple?.body).toBeUndefined();
  });
  it("shows the system's words for a microphone error", () => {
    const mic = panelView("idle", message({ key: "widget.microphone_unavailable", detail: "device gone" }), "", null, false);
    expect(mic).toEqual({ title: "無法使用麥克風", body: t("panel.microphone"), detail: "device gone", actions: [], close: false });
  });
  it("offers the text that was not inserted", () => {
    const shown = panelView("idle", message({ kind: "info", key: "widget.app_changed", copyable: "聽寫結果" }), "", null, false);
    expect(shown).toEqual({ title: t("widget.app_changed"), result: "聽寫結果", actions: ["copy"], close: true });
  });
  it("shows the reason of a failed rewrite", () => {
    const degraded = panelView("idle", message({ key: "widget.degraded", detail: "Ollama: timed out" }), "", null, false);
    expect(degraded).toEqual({ title: t("widget.degraded"), detail: "Ollama: timed out", actions: [], close: false });
  });
});

describe("settings window line", () => {
  it("names a dictation with the widget's labels", () => {
    expect(dictationLabelKey("recording")).toBe(labelKey("recording", null));
    expect(dictationLabelKey("processing")).toBe(labelKey("processing", null));
    expect(dictationLabelKey("inserting")).toBe(labelKey("inserting", null));
    expect(dictationLabelKey("locked")).toBe("shell.hands_free");
  });

  it("has nothing to say when no dictation goes on", () => {
    for (const state of ["idle", "placing", "no_accessibility", "engine_error"] as const) expect(dictationLabelKey(state)).toBeNull();
  });
});
