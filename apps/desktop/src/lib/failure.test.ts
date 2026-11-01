import { describe, expect, it } from "vitest";
import { failureText, stageFailure, serviceWords } from "./failure";
import { setLanguage } from "./i18n";

describe("failure texts", () => {
  it("uses the text of a known kind and falls back to the detail", () => {
    setLanguage("zh-TW");
    expect(failureText("provider_timeout", "Speaches: request timed out")).toBe("引擎沒有在時限內回應。");
    expect(failureText("something_new", "raw detail")).toBe("raw detail");
    expect(failureText(null, "raw detail")).toBe("raw detail");
    expect(failureText(null, null)).toBe("");
  });

  it("puts the status before the service's own words and keeps its answer", () => {
    expect(serviceWords({ status: 429, answer: "slow down", raw: "{}" })).toEqual({ body: "HTTP 429: slow down", detail: "{}" });
    expect(serviceWords({ answer: "slow down" })).toEqual({ body: "slow down", detail: null });
    expect(serviceWords({ status: 500 })).toEqual({ body: "HTTP 500", detail: null });
    expect(serviceWords({ error: "provider_error" })).toEqual({ body: null, detail: null });
  });

  it("finds what the service said in a failed row's stages", () => {
    const failed = { position: 1, kind: "asr" as const, text: null, ms: null, detail: { error: "provider_error", status: 429, answer: "slow down", raw: "{}" } };
    expect(stageFailure([failed])).toEqual({ body: "HTTP 429: slow down", detail: "{}" });
    expect(stageFailure([{ ...failed, detail: {} }])).toEqual({ body: null, detail: null });
    expect(stageFailure(null)).toEqual({ body: null, detail: null });
  });
});
