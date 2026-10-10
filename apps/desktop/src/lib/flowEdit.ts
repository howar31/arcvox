import type { Config, Profile, StepSpec } from "./api";

const copy = (steps: StepSpec[]): StepSpec[] => steps.map((s) => ({ ...s }));

/** The profile the page shows: the one named by `arg`, else the default, else the first. */
export function selectedProfile(config: Config | null, arg: string | null): Profile | null {
  const profiles = config?.profiles ?? [];
  return profiles.find((p) => p.id === arg) ?? profiles.find((p) => p.is_default) ?? profiles[0] ?? null;
}

/** True when the profile has a script conversion step. */
export function convertsScript(profile: Profile): boolean {
  return profile.steps.some((s) => s.step === "opencc");
}

/** The conversions the page offers: characters only, or characters and Taiwan phrasing. */
export type ScriptKind = "tw" | "twp";
const CONFIG: Record<ScriptKind, string> = { tw: "s2tw", twp: "s2twp" };

/** The conversion a profile applies: characters only, characters and phrasing, none, or the name of another configuration. */
export function scriptChoice(profile: Profile): ScriptKind | "none" | { other: string } {
  const step = profile.steps.find((s) => s.step === "opencc");
  if (!step) return "none";
  const config = step.config ?? CONFIG.tw;
  if (config === CONFIG.tw) return "tw";
  if (config === CONFIG.twp) return "twp";
  return { other: config };
}

/**
 * A copy with the chosen conversion: "none" removes every conversion step; a kind
 * sets it on every conversion step the profile has, or, when it has none, adds one
 * after speech recognition and one after the rewrite.
 */
export function withScript(profile: Profile, choice: ScriptKind | "none"): Profile {
  let steps = copy(profile.steps);
  if (choice === "none") return { ...profile, steps: steps.filter((s) => s.step !== "opencc") };
  const config = CONFIG[choice];
  if (convertsScript(profile)) {
    steps = steps.map((s) => (s.step === "opencc" ? { ...s, config } : s));
    return { ...profile, steps };
  }
  const llm = steps.findIndex((s) => s.step === "llm");
  if (llm >= 0) steps.splice(llm + 1, 0, { step: "opencc", config });
  steps.splice(steps.findIndex((s) => s.step === "asr") + 1, 0, { step: "opencc", config });
  return { ...profile, steps };
}

/** True when the profile applies the dictionary's aliases to the text. */
export function appliesDictionary(profile: Profile): boolean {
  return profile.steps.some((s) => s.step === "rules");
}

/** A copy without any dictionary step, or with one added at the end when there was none. */
export function withDictionary(profile: Profile, on: boolean): Profile {
  const steps = copy(profile.steps);
  if (!on) return { ...profile, steps: steps.filter((s) => s.step !== "rules") };
  if (!appliesDictionary(profile)) steps.push({ step: "rules" });
  return { ...profile, steps };
}
