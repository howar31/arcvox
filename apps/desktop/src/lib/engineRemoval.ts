// Deleting network engines: the profiles that name them move first, then the engines and their keys go.

import { deleteProvider, deleteSecret, upsertProfile, type PresetView, type Profile, type ProviderConfig } from "./api";
import { namesEngine, templateEngines, withoutEngine } from "./profileEdit";
import { config } from "./store.svelte";

export type MoveTo = "default" | "template";

/** The profiles that name any of the engines. */
export const namingProfiles = (profiles: Profile[], ids: string[]): Profile[] => profiles.filter((p) => ids.some((id) => namesEngine(p, id)));

/** Whether a template names an engine of its own for a stage that now names one of the engines. */
export const templateOffers = (profiles: Profile[], ids: string[], presets: PresetView[]): boolean =>
  profiles.some((p) => ids.some((id) => {
    const to = templateEngines(p, id, presets);
    return (p.asr_provider === id && !!to.asr) || (p.llm_provider === id && !!to.llm);
  }));

/** A profile with every one of the engines off its stages. */
export function moved(profile: Profile, ids: string[], moveTo: MoveTo, presets: PresetView[]): Profile {
  let next = profile;
  for (const id of ids) {
    if (!namesEngine(next, id)) continue;
    next = withoutEngine(next, id, moveTo === "template" ? templateEngines(next, id, presets) : { asr: null, llm: null });
  }
  return next;
}

/** Deletes the engines; returns the ids that were deleted before any failure, which is thrown after. */
export async function removeEngines(engines: ProviderConfig[], moveTo: MoveTo, presets: PresetView[]): Promise<string[]> {
  const ids = engines.map((e) => e.id);
  for (const p of namingProfiles(config.value?.profiles ?? [], ids)) {
    config.value = await upsertProfile(moved(p, ids, moveTo, presets));
  }
  const done: string[] = [];
  for (const e of engines) {
    await deleteProvider(e.id);
    if (e.api_key_ref) await deleteSecret(e.api_key_ref).catch(() => {});
    done.push(e.id);
  }
  return done;
}
