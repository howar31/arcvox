import { describe, expect, it } from "vitest";
import { failureText } from "./failure";
import { setLanguage } from "./i18n";

describe("failure texts", () => {
  it("uses the text of a known kind and falls back to the detail", () => {
    setLanguage("zh-TW");
    expect(failureText("provider_timeout", "Speaches: request timed out")).toBe("引擎沒有在時限內回應。");
    expect(failureText("something_new", "raw detail")).toBe("raw detail");
    expect(failureText(null, "raw detail")).toBe("raw detail");
    expect(failureText(null, null)).toBe("");
  });
});
