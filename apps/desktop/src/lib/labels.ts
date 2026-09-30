import type { Config, ManifestView, Profile, ProviderConfig } from "./api";

export function isKnownProvider(config: Config | null, id: string | null): boolean {
  return id !== null && !!config?.providers.some((p) => p.id === id);
}

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

/** What an embedded provider loads: the catalog label, the file name, or the provider label. */
export function modelLabel(manifest: ManifestView | null, provider: ProviderConfig): string {
  if (provider.model_id) return manifest?.models.find((m) => m.id === provider.model_id)?.label ?? provider.model_id;
  if (provider.model_path) return provider.model_path.split(/[\\/]/).pop() || provider.model_path;
  return provider.label;
}
