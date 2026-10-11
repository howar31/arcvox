import type { Config, Profile, StepSpec } from "./api";
import { t } from "./i18n";

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

/** Every OpenCC configuration the core accepts, grouped by the script it produces, in the order the page lists them. */
export const SCRIPT_GROUPS: { id: "tw" | "hk" | "t" | "s" | "jp"; configs: string[] }[] = [
  { id: "tw", configs: ["s2tw", "s2twp", "t2tw", "t2twp"] },
  { id: "hk", configs: ["s2hk", "t2hk"] },
  { id: "t", configs: ["s2t", "tw2t", "tw2tp", "hk2t", "jp2t"] },
  { id: "s", configs: ["t2s", "tw2s", "tw2sp", "hk2s"] },
  { id: "jp", configs: ["t2jp"] },
];
export const SCRIPT_CONFIGS: string[] = SCRIPT_GROUPS.flatMap((g) => g.configs);
/** The configurations that can convert quotation marks too; the regional ones ignore the choice. */
export const PUNCTUATION_CONFIGS = ["s2t", "s2tw", "s2twp", "s2hk", "t2s", "tw2s", "tw2sp", "hk2s"];

/** The two places a conversion runs: after speech recognition, and after the rewrite. */
export type ScriptPass = 1 | 2;
export interface ScriptStep { config: string; punctuation: boolean }

/** The index of the conversion step for a pass: the first one before the rewrite, or the first one after it. */
function scriptIndex(profile: Profile, pass: ScriptPass): number {
  const llm = profile.steps.findIndex((s) => s.step === "llm");
  return profile.steps.findIndex((s, i) => s.step === "opencc" && (pass === 1 ? i < llm || llm < 0 : llm >= 0 && i > llm));
}

/** The conversion one pass applies, or null when that pass is off. */
export function scriptAt(profile: Profile, pass: ScriptPass): ScriptStep | null {
  const step = profile.steps[scriptIndex(profile, pass)];
  if (!step || step.step !== "opencc") return null;
  return { config: step.config, punctuation: step.punctuation ?? false };
}

/**
 * A copy with the configuration of one pass set: null removes that pass's step; a name sets it (keeping the
 * punctuation choice), or adds the step after speech recognition (pass 1) or after the rewrite (pass 2, at
 * the end when there is no rewrite).
 */
export function withScriptAt(profile: Profile, pass: ScriptPass, config: string | null): Profile {
  const steps = copy(profile.steps);
  const i = scriptIndex(profile, pass);
  if (config === null) {
    if (i >= 0) steps.splice(i, 1);
    return { ...profile, steps };
  }
  const was = steps[i];
  const punctuation = was?.step === "opencc" ? (was.punctuation ?? false) : false;
  if (i >= 0) { steps[i] = { step: "opencc", config, punctuation }; return { ...profile, steps }; }
  const llm = steps.findIndex((s) => s.step === "llm");
  const at = pass === 1 ? steps.findIndex((s) => s.step === "asr") + 1 : llm >= 0 ? llm + 1 : steps.length;
  steps.splice(at, 0, { step: "opencc", config, punctuation });
  return { ...profile, steps };
}

/** A copy with the punctuation choice of one pass set; unchanged when that pass is off. */
export function withScriptPunctuation(profile: Profile, pass: ScriptPass, on: boolean): Profile {
  const steps = copy(profile.steps);
  const i = scriptIndex(profile, pass);
  const step = steps[i];
  if (!step || step.step !== "opencc") return profile;
  steps[i] = { ...step, punctuation: on };
  return { ...profile, steps };
}

/** A configuration in words, or its name when the page has none for it. */
export function scriptLabel(config: string): string {
  return SCRIPT_CONFIGS.includes(config) ? t(`flow.script.${config}`) : config;
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

