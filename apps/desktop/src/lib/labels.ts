import type { Config, InstalledModel, ManifestView, ModelEntry, Profile } from "./api";
import { has, t } from "./i18n";

export function providerLabel(config: Config | null, id: string | null): string {
  if (id === null) return "";
  return config?.providers.find((p) => p.id === id)?.label ?? id;
}

export function profileName(config: Config | null, id: string): string {
  return config?.profiles.find((p) => p.id === id)?.name ?? id;
}

export function defaultProfile(config: Config | null): Profile | null {
  return config?.profiles.find((p) => p.is_default) ?? null;
}

/** Profiles that select the provider for speech or rewrite, the default profile first. */
export function usedBy(config: Config | null, providerId: string): { name: string; isDefault: boolean }[] {
  const users = (config?.profiles ?? []).filter((p) => p.asr_provider === providerId || p.llm_provider === providerId);
  return users
    .map((p) => ({ name: p.name, isDefault: p.is_default }))
    .sort((a, b) => Number(b.isDefault) - Number(a.isDefault));
}

/** An installed model's name: the file name when imported, else its catalogue label or its id. */
export function modelLabel(manifest: ManifestView | null, model: InstalledModel): string {
  if (model.custom) return model.path.split(/[\\/]/).pop() || model.path;
  return manifest?.models.find((m) => m.id === model.id)?.label ?? model.id;
}

/** A catalogue model's description in the interface language; the catalogue's own text for models without one. */
export function modelDescription(entry: Pick<ModelEntry, "id" | "description">): string {
  const key = `engines.model_desc.${entry.id}`;
  return has(key) ? t(key) : entry.description;
}
