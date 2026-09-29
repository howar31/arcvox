import { describe, expect, it } from "vitest";
import { blankFor, describeProbe } from "./providerForm";

describe("providerForm", () => {
  it("blank embedded provider has runtime and no base url", () => {
    const p = blankFor("embedded");
    expect(p.kind).toBe("embedded");
    expect(p.runtime).toBe("whisper");
    expect(p.base_url).toBeNull();
    expect(p.capabilities.transcribe).toBe(true);
  });
  it("blank native provider points at apple dictation", () => {
    expect(blankFor("native").native_id).toBe("apple.dictation");
  });
  it("describeProbe names model and native failures", () => {
    const t = (k: string) => k;
    expect(describeProbe({ ok: false, failure: { kind: "model_missing" } }, t)).toBe("providers.probe_failed: providers.failure.model_missing");
    expect(describeProbe({ ok: false, failure: { kind: "native_unavailable", reason: "off" } }, t)).toContain("off");
    expect(describeProbe({ ok: false, failure: { kind: "native_unavailable", reason: "off" } }, t)).toBe("providers.probe_failed: providers.failure.native_unavailable (off)");
    expect(describeProbe({ ok: true, failure: null }, t)).toBe("providers.probe_ok");
  });
});
