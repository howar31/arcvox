import { describe, expect, it } from "vitest";
import type { Config, Profile } from "./api";
import { appliesDictionary, convertsScript, scriptChoice, selectedProfile, withDictionary, withScript } from "./flowEdit";

const base: Profile = {
  id: "default", name: "Default", is_default: true, matcher: { app_ids: [], url_patterns: [] },
  asr_provider: "a", llm_provider: "l", language_hint: null, prompt_addition: "", few_shot: [],
  steps: [{ step: "asr" }, { step: "opencc", config: "s2tw" }, { step: "llm" }, { step: "opencc", config: "s2tw" }, { step: "rules" }],
};
const kinds = (p: Profile) => p.steps.map((s) => s.step);

describe("flowEdit", () => {
  it("turns script conversion off by removing every such step and on by adding one after recognition and one after the rewrite", () => {
    const off = withScript(base, "none");
    expect(kinds(off)).toEqual(["asr", "llm", "rules"]);
    expect(convertsScript(off)).toBe(false);
    const on = withScript(off, "tw");
    expect(kinds(on)).toEqual(["asr", "opencc", "llm", "opencc", "rules"]);
    expect(on.steps[1]).toEqual({ step: "opencc", config: "s2tw" });
    expect(on.steps[3]).toEqual({ step: "opencc", config: "s2tw" });
    const noLlm: Profile = { ...off, steps: [{ step: "asr" }, { step: "rules" }] };
    expect(kinds(withScript(noLlm, "twp"))).toEqual(["asr", "opencc", "rules"]);
  });

  it("names the conversion a profile applies and changes it on every conversion step", () => {
    expect(scriptChoice(base)).toBe("tw");
    expect(scriptChoice(withScript(base, "none"))).toBe("none");
    const twp = withScript(base, "twp");
    expect(scriptChoice(twp)).toBe("twp");
    expect(twp.steps.filter((s) => s.step === "opencc").map((s) => s.config)).toEqual(["s2twp", "s2twp"]);
    const hk: Profile = { ...base, steps: [{ step: "asr" }, { step: "opencc", config: "s2hk" }] };
    expect(scriptChoice(hk)).toEqual({ other: "s2hk" });
    expect(scriptChoice(withScript(hk, "tw"))).toBe("tw");
  });

  it("turns the dictionary off and on at the end", () => {
    const off = withDictionary(base, false);
    expect(appliesDictionary(off)).toBe(false);
    expect(kinds(withDictionary(off, true)).at(-1)).toBe("rules");
    expect(kinds(withDictionary(base, true))).toEqual(kinds(base));
  });

  it("does not change the profile it was given", () => {
    withScript(base, "none");
    withDictionary(base, false);
    expect(base.steps).toHaveLength(5);
  });

  it("selects the named profile, else the default, else the first", () => {
    const other = { ...base, id: "mail", is_default: false };
    const config = { profiles: [other, base] } as unknown as Config;
    expect(selectedProfile(config, "mail")?.id).toBe("mail");
    expect(selectedProfile(config, "gone")?.id).toBe("default");
    expect(selectedProfile(config, null)?.id).toBe("default");
    expect(selectedProfile({ profiles: [other] } as unknown as Config, null)?.id).toBe("mail");
    expect(selectedProfile(null, null)).toBeNull();
  });
});
