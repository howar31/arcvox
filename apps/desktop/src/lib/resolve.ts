// Which engines a profile runs with now: the one it names when that one is there and can
// run, else the default pair. The same rules as the core's resolution; the stored value is never changed.

import type { Config, Profile, ProviderConfig } from "./api";
import { engineMeta } from "./profileEdit";

/** The picker's value for a profile that names no engine and follows the default. */
export const FOLLOW = "";

export type Role = "asr" | "llm";

export type Source =
  | { kind: "explicit" }
  | { kind: "default" }
  /** The profile names `wanted`, which is gone or cannot run; the default is used until it is back. */
  | { kind: "fallback"; wanted: string };

export interface Choice { id: string; source: Source }

/** `llm` is null when the profile has no rewrite step, `translate` null when it has no translation. */
export interface Resolved { asr: Choice; llm: Choice | null; translate: Choice | null }

export const hasLlmStep = (profile: Pick<Profile, "steps">): boolean => profile.steps.some((s) => s.step === "llm");

/** Whether an engine can run now, from the installed model files; platform and network engines are taken at their word. */
export function usableWith(installedIds: Set<string> | null, installedPaths?: Set<string>): (p: ProviderConfig) => boolean {
  return (p) => installedIds === null || engineMeta(p, installedIds, installedPaths).ready;
}

export function resolveEngines(config: Config | null, profile: Profile, usable: (p: ProviderConfig) => boolean): Resolved {
  const choose = (named: string | null, role: Role): Choice => {
    const fallback = config?.default_engines[role] ?? "";
    if (named === null) return { id: fallback, source: { kind: "default" } };
    const engine = config?.providers.find((p) => p.id === named);
    if (engine && usable(engine)) return { id: named, source: { kind: "explicit" } };
    return { id: fallback, source: { kind: "fallback", wanted: named } };
  };
  const llm = hasLlmStep(profile) ? choose(profile.llm_provider, "llm") : null;
  const translation = profile.steps.find((s) => s.step === "translate");
  // The translation follows the rewrite engine unless it names one; without a rewrite it follows the default rewrite engine.
  const translate = !translation || translation.step !== "translate" ? null
    : translation.provider ? choose(translation.provider, "llm")
    : llm ? { id: llm.id, source: { kind: "default" } as Source }
    : choose(null, "llm");
  return { asr: choose(profile.asr_provider, "asr"), llm, translate };
}

export const isFallback = (c: Choice | null): c is Choice & { source: { kind: "fallback"; wanted: string } } =>
  c !== null && c.source.kind === "fallback";

/** Whether either role runs on the default instead of a named engine. */
export const anyFallback = (r: Resolved): boolean => isFallback(r.asr) || isFallback(r.llm) || isFallback(r.translate);
