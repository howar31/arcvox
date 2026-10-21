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
 * punctuation choice), or adds the step after speech recognition (pass 1) or after the rewrite (pass 2).
 * Without a rewrite there is no second pass, so the profile comes back as it is.
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
  if (pass === 2 && llm < 0) return profile;
  const at = pass === 1 ? steps.findIndex((s) => s.step === "asr") + 1 : llm + 1;
  steps.splice(at, 0, { step: "opencc", config, punctuation });
  return { ...profile, steps };
}

/** A copy with the output script set for both places: after recognition, and after the rewrite when there is one; null removes both. */
export function withScript(profile: Profile, config: string | null): Profile {
  return withScriptAt(withScriptAt(profile, 1, config), 2, config);
}

/** A copy with the punctuation choice set for both places. */
export function withScriptPunctuationBoth(profile: Profile, on: boolean): Profile {
  return withScriptPunctuation(withScriptPunctuation(profile, 1, on), 2, on);
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

export type RulesStep = Extract<StepSpec, { step: "rules" }>;
export type RuleKey = "dictionary" | "trailing_stop" | "question_mark" | "question_mark_inside" | "spacing" | "punctuation_width";
export const RULE_KEYS: RuleKey[] = ["dictionary", "trailing_stop", "question_mark", "question_mark_inside", "spacing", "punctuation_width"];

/** The rules step's switches, whether the module is on or off: a step from before the text rules has the dictionary on, the trailing stop kept and the other rules on. */
export type RuleSwitches = Required<Omit<RulesStep, "off">>;
export function rulesAt(profile: Profile): RuleSwitches | null {
  const s = profile.steps.find((s): s is RulesStep => s.step === "rules");
  if (!s) return null;
  return { step: "rules", dictionary: s.dictionary ?? true, trailing_stop: s.trailing_stop ?? false, question_mark: s.question_mark ?? true, question_mark_inside: s.question_mark_inside ?? true, spacing: s.spacing ?? true, punctuation_width: s.punctuation_width ?? true };
}

/** True when the profile applies the dictionary's aliases to the text: the tidy module is on with its dictionary switch. */
export function appliesDictionary(profile: Profile): boolean {
  const s = profile.steps.find((s) => s.step === "rules");
  return !!s && !("off" in s && s.off) && (rulesAt(profile)?.dictionary ?? false);
}

/** A copy with one switch of the rules step set (an off module stays off); a profile without the step gets one at the end, its other switches at their defaults. */
export function withRule(profile: Profile, key: RuleKey, on: boolean): Profile {
  const steps = copy(profile.steps);
  const current = rulesAt(profile);
  const next: RuleSwitches = { ...(current ?? { step: "rules", dictionary: true, trailing_stop: false, question_mark: true, question_mark_inside: true, spacing: true, punctuation_width: true }), [key]: on };
  const i = steps.findIndex((s) => s.step === "rules");
  if (i >= 0) steps[i] = (steps[i] as RulesStep).off ? { ...next, off: true } : next; else steps.push(next);
  return { ...profile, steps };
}

/** A copy with the tidy module enabled or turned off. A step that is there keeps its switches either way; a profile without one gets it (switches at their defaults) after the rewrite and its conversion, before a translation. */
export function withRules(profile: Profile, on: boolean): Profile {
  const steps: StepSpec[] = copy(profile.steps);
  const i = steps.findIndex((s) => s.step === "rules");
  if (i >= 0) {
    const { off: _off, ...rest } = steps[i] as RulesStep;
    steps[i] = on ? rest : { ...rest, off: true };
  } else if (on) {
    const translate = steps.findIndex((s) => s.step === "translate");
    const at = translate >= 0 ? translate : steps.length;
    steps.splice(at, 0, { step: "rules", dictionary: true, trailing_stop: false, question_mark: true, question_mark_inside: true, spacing: true, punctuation_width: true });
  }
  return { ...profile, steps };
}

export type TranslateStep = Extract<StepSpec, { step: "translate" }>;

/** The translation module's step, or null when the profile has none. */
export function translateAt(profile: Profile): TranslateStep | null {
  const s = profile.steps.find((s): s is TranslateStep => s.step === "translate");
  return s ?? null;
}

/** A copy with the translation module set (added at the end when missing) or, with null, turned off with its settings kept. */
export function withTranslate(profile: Profile, step: TranslateStep | null): Profile {
  const steps: StepSpec[] = copy(profile.steps);
  const i = steps.findIndex((s) => s.step === "translate");
  if (step) {
    const { off: _off, ...rest } = step;
    if (i >= 0) steps[i] = { ...rest }; else steps.push({ ...rest });
  } else if (i >= 0) {
    steps[i] = { ...(steps[i] as TranslateStep), off: true };
  }
  return { ...profile, steps };
}

/** A copy with the translation module enabled: the step it has, settings kept, or a new one with `target`. */
export function enableTranslate(profile: Profile, target: string): Profile {
  const s = translateAt(profile);
  return withTranslate(profile, s ? { ...s } : { step: "translate", target });
}

/** A copy with the dictionary switch of the rules step set. */
export function withDictionary(profile: Profile, on: boolean): Profile {
  return withRule(profile, "dictionary", on);
}

