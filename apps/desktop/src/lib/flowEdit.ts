import type { Config, Profile, StepSpec } from "./api";

const copy = (steps: StepSpec[]): StepSpec[] => steps.map((s) => ({ ...s }));

/** The profile the page shows: the one named by `arg`, else the default, else the first. */
export function selectedProfile(config: Config | null, arg: string | null): Profile | null {
  const profiles = config?.profiles ?? [];
  return profiles.find((p) => p.id === arg) ?? profiles.find((p) => p.is_default) ?? profiles[0] ?? null;
}

/** True when the profile converts the text to Taiwan Traditional. */
export function convertsScript(profile: Profile): boolean {
  return profile.steps.some((s) => s.step === "opencc");
}

/** A copy without any conversion step, or with one added after speech recognition when there was none. */
export function withScript(profile: Profile, on: boolean): Profile {
  const steps = copy(profile.steps);
  if (!on) return { ...profile, steps: steps.filter((s) => s.step !== "opencc") };
  if (!convertsScript(profile)) steps.splice(steps.findIndex((s) => s.step === "asr") + 1, 0, { step: "opencc", config: "s2twp" });
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
