import type { Profile, ProviderConfig, StepSpec } from "./api";

/**
 * Returns a copy of the profile that rewrites with `llmId`, or does not rewrite
 * when it is null. Turning rewriting off drops every `llm` step; turning it on
 * when there is no `llm` step inserts one after the first OpenCC step, or after
 * speech recognition when there is none.
 */
export function withLlm(profile: Profile, llmId: string | null): Profile {
  const steps: StepSpec[] = profile.steps.map((s) => ({ ...s }));
  if (llmId === null) return { ...profile, llm_provider: null, steps: steps.filter((s) => s.step !== "llm") };
  if (!steps.some((s) => s.step === "llm")) {
    let at = steps.findIndex((s) => s.step === "opencc");
    if (at < 0) at = steps.findIndex((s) => s.step === "asr");
    steps.splice(at + 1, 0, { step: "llm" });
  }
  return { ...profile, llm_provider: llmId, steps };
}

/** The first `max` names and how many more there are. */
export function appsSummary(names: string[], max = 3): { shown: string[]; more: number } {
  return { shown: names.slice(0, max), more: Math.max(0, names.length - max) };
}

export interface EngineMeta {
  kind: "local" | "builtin" | "external";
  /** False when a local engine's model file is not installed. */
  ready: boolean;
  /** Host of an external engine. */
  detail: string | null;
}

/**
 * Where an engine runs and whether it can run now. `installedPaths`, when given, also checks an imported
 * file by path; without it a set path counts as ready.
 */
export function engineMeta(provider: ProviderConfig, installedIds: Set<string>, installedPaths?: Set<string>): EngineMeta {
  if (provider.kind === "native") return { kind: "builtin", ready: true, detail: null };
  if (provider.kind === "http") return { kind: "external", ready: true, detail: hostOf(provider.base_url) };
  const path = provider.model_path;
  const ready = provider.model_id ? installedIds.has(provider.model_id) : path !== null && (!installedPaths || installedPaths.has(path));
  return { kind: "local", ready, detail: null };
}

function hostOf(url: string | null): string | null {
  if (!url) return null;
  try { return new URL(url).host || url; } catch { return url; }
}

/** Route argument of a profile that is not saved yet; "+" never appears in an id from `slugId`. */
export const NEW_PROFILE = "+new";
/** Ids `slugId` never returns: "new" was the route argument of an unsaved profile in earlier builds. */
const RESERVED_IDS = ["new", NEW_PROFILE];

/** A new profile that starts from the engines, language and steps of `base`. */
export function blankProfile(base: Profile): Profile {
  return {
    id: "", name: "", is_default: false, matcher: { app_ids: [], url_patterns: [] },
    asr_provider: base.asr_provider, llm_provider: base.llm_provider, language_hint: base.language_hint,
    prompt_addition: "", few_shot: [], steps: base.steps.map((s) => ({ ...s })),
  };
}

/**
 * An id for a new profile: the lowercase ASCII words of the name joined by dashes, with a
 * numeric suffix when taken or reserved ("new"). A name without ASCII letters or digits becomes `profile-<n>`.
 */
export function slugId(name: string, taken: string[]): string {
  const used = new Set([...taken, ...RESERVED_IDS]);
  const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  if (slug && !used.has(slug)) return slug;
  const stem = slug || "profile";
  let n = slug ? 2 : 1;
  while (used.has(`${stem}-${n}`)) n++;
  return `${stem}-${n}`;
}

/**
 * Returns a copy of the steps with the step at `index` moved by `delta` places. Moves past either
 * end, and moves that would take speech recognition out of the first place, return the steps unchanged.
 */
export function moveStep(steps: StepSpec[], index: number, delta: number): StepSpec[] {
  const next = steps.map((s) => ({ ...s }));
  const to = index + delta;
  if (index < 0 || index >= next.length || to < 0 || to >= next.length) return next;
  if (next[index].step === "asr" || next[to].step === "asr") return next;
  [next[index], next[to]] = [next[to], next[index]];
  return next;
}
