import { describe, expect, it } from "vitest";
import type { Config, Profile } from "./api";
import { appliesDictionary, convertsScript, rulesAt, SCRIPT_CONFIGS, scriptAt, scriptLabel, selectedProfile, withDictionary, withRule, withScriptAt, withScriptPunctuation } from "./flowEdit";

const base: Profile = {
  id: "default", name: "Default", is_default: true, matcher: { app_ids: [], url_patterns: [] },
  asr_provider: "a", llm_provider: "l", language_hint: null, prompt_addition: "", few_shot: [], preset: null, preset_revision: 0, asr_dictionary_hint: true, llm_dictionary_terms: true, llm_window_title: true, llm_short_fallback: true,
  steps: [{ step: "asr" }, { step: "opencc", config: "s2tw" }, { step: "llm" }, { step: "opencc", config: "s2tw" }, { step: "rules" }],
};
const kinds = (p: Profile) => p.steps.map((s) => s.step);

describe("flowEdit", () => {
  it("reads and sets each conversion pass on its own", () => {
    expect(scriptAt(base, 1)).toEqual({ config: "s2tw", punctuation: false });
    expect(scriptAt(base, 2)).toEqual({ config: "s2tw", punctuation: false });
    const second = withScriptAt(base, 2, "s2twp");
    expect(kinds(second)).toEqual(["asr", "opencc", "llm", "opencc", "rules"]);
    expect(second.steps[1]).toEqual({ step: "opencc", config: "s2tw" });
    expect(second.steps[3]).toEqual({ step: "opencc", config: "s2twp", punctuation: false });
    expect(scriptAt(second, 2)?.config).toBe("s2twp");
    const noFirst = withScriptAt(base, 1, null);
    expect(kinds(noFirst)).toEqual(["asr", "llm", "opencc", "rules"]);
    expect(scriptAt(noFirst, 1)).toBeNull();
    expect(scriptAt(noFirst, 2)?.config).toBe("s2tw");
    const noSecond = withScriptAt(base, 2, null);
    expect(kinds(noSecond)).toEqual(["asr", "opencc", "llm", "rules"]);
    expect(scriptAt(noSecond, 2)).toBeNull();
    expect(kinds(withScriptAt(noFirst, 1, "t2jp"))).toEqual(["asr", "opencc", "llm", "opencc", "rules"]);
    // Without a rewrite there is no second pass to set.
    const noLlm: Profile = { ...base, steps: [{ step: "asr" }, { step: "rules" }] };
    expect(kinds(withScriptAt(noLlm, 2, "s2hk"))).toEqual(["asr", "rules"]);
    expect(kinds(withScriptAt(noLlm, 1, "s2hk"))).toEqual(["asr", "opencc", "rules"]);
    expect(scriptAt(noLlm, 2)).toBeNull();
    expect(convertsScript(noLlm)).toBe(false);
  });

  it("keeps and sets the punctuation choice of a pass", () => {
    const on = withScriptPunctuation(base, 1, true);
    expect(scriptAt(on, 1)).toEqual({ config: "s2tw", punctuation: true });
    expect(scriptAt(on, 2)?.punctuation).toBe(false);
    expect(scriptAt(withScriptAt(on, 1, "s2hk"), 1)).toEqual({ config: "s2hk", punctuation: true });
    expect(withScriptPunctuation(withScriptAt(base, 1, null), 1, true)).toEqual(withScriptAt(base, 1, null));
  });

  it("names every configuration the core accepts and shows an unknown one by its name", () => {
    expect(SCRIPT_CONFIGS).toHaveLength(16);
    expect(new Set(SCRIPT_CONFIGS).size).toBe(16);
    expect(scriptLabel("s2tw")).not.toBe("s2tw");
    expect(scriptLabel("x2y")).toBe("x2y");
  });

  it("turns the dictionary off and on at the end", () => {
    const off = withDictionary(base, false);
    expect(appliesDictionary(off)).toBe(false);
    expect(kinds(withDictionary(off, true)).at(-1)).toBe("rules");
    expect(kinds(withDictionary(base, true))).toEqual(kinds(base));
  });

  it("reads a rules step from before the text rules with the trailing stop kept", () => {
    expect(rulesAt(base)).toEqual({ step: "rules", dictionary: true, trailing_stop: false, question_mark: true, question_mark_inside: true, spacing: true, punctuation_width: true });
    expect(rulesAt({ ...base, steps: [{ step: "asr" }] })).toBeNull();
  });

  it("sets one rule at a time and keeps the step in place", () => {
    const on = withRule(base, "trailing_stop", true);
    expect(rulesAt(on)?.trailing_stop).toBe(true);
    expect(rulesAt(on)?.spacing).toBe(true);
    expect(kinds(on)).toEqual(kinds(base));
    const off = withRule(on, "spacing", false);
    expect(rulesAt(off)).toEqual({ step: "rules", dictionary: true, trailing_stop: true, question_mark: true, question_mark_inside: true, spacing: false, punctuation_width: true });
    const added = withRule({ ...base, steps: [{ step: "asr" }] }, "question_mark", false);
    expect(kinds(added)).toEqual(["asr", "rules"]);
    expect(rulesAt(added)?.question_mark).toBe(false);
  });

  it("does not change the profile it was given", () => {
    withScriptAt(base, 1, null);
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
