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

/** Where an engine runs and whether it can run now. */
export function engineMeta(provider: ProviderConfig, installedIds: Set<string>): EngineMeta {
  if (provider.kind === "native") return { kind: "builtin", ready: true, detail: null };
  if (provider.kind === "http") return { kind: "external", ready: true, detail: hostOf(provider.base_url) };
  const ready = provider.model_id ? installedIds.has(provider.model_id) : provider.model_path !== null;
  return { kind: "local", ready, detail: null };
}

function hostOf(url: string | null): string | null {
  if (!url) return null;
  try { return new URL(url).host || url; } catch { return url; }
}
