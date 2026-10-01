import { describe, expect, it } from "vitest";
import type { Config, Profile } from "./api";
import { appliesDictionary, convertsScript, selectedProfile, withDictionary, withScript } from "./flowEdit";

const base: Profile = {
  id: "default", name: "Default", is_default: true, matcher: { app_ids: [], url_patterns: [] },
  asr_provider: "a", llm_provider: "l", language_hint: null, prompt_addition: "", few_shot: [],
  steps: [{ step: "asr" }, { step: "opencc", config: "s2twp" }, { step: "llm" }, { step: "opencc", config: "s2twp" }, { step: "rules" }],
};
const kinds = (p: Profile) => p.steps.map((s) => s.step);

describe("flowEdit", () => {
  it("turns script conversion off by removing every such step and on by adding one after recognition", () => {
    const off = withScript(base, false);
    expect(kinds(off)).toEqual(["asr", "llm", "rules"]);
    expect(convertsScript(off)).toBe(false);
    const on = withScript(off, true);
    expect(kinds(on)).toEqual(["asr", "opencc", "llm", "rules"]);
    expect(on.steps[1]).toEqual({ step: "opencc", config: "s2twp" });
    expect(kinds(withScript(base, true))).toEqual(kinds(base));
  });

  it("turns the dictionary off and on at the end", () => {
    const off = withDictionary(base, false);
    expect(appliesDictionary(off)).toBe(false);
    expect(kinds(withDictionary(off, true)).at(-1)).toBe("rules");
    expect(kinds(withDictionary(base, true))).toEqual(kinds(base));
  });

  it("does not change the profile it was given", () => {
    withScript(base, false);
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
