import type { Profile, ProviderConfig, StepSpec } from "./api";

/**
 * Returns a copy of the profile that rewrites with `pick`: an engine id, or `FOLLOW` ("") to
 * rewrite with the default engine; null does not rewrite at all. Turning rewriting off drops
 * every `llm` step; turning it on when there is no `llm` step inserts one after the first
 * OpenCC step, or after speech recognition when there is none.
 */
export function withLlm(profile: Profile, pick: string | null): Profile {
  const steps: StepSpec[] = profile.steps.map((s) => ({ ...s }));
  if (pick === null) return { ...profile, llm_provider: null, steps: steps.filter((s) => s.step !== "llm") };
  if (!steps.some((s) => s.step === "llm")) {
    let at = steps.findIndex((s) => s.step === "opencc");
    if (at < 0) at = steps.findIndex((s) => s.step === "asr");
    steps.splice(at + 1, 0, { step: "llm" });
  }
  return { ...profile, llm_provider: pick === "" ? null : pick, steps };
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

/** Ids `slugId` never returns: earlier builds used them as the route argument of an unsaved profile. */
const RESERVED_IDS = ["new", "+new"];

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
