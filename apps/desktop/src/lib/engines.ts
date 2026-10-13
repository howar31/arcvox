import type { Config, InstalledModel, ManifestView, ModelEntry, ProviderConfig, Runtime } from "./api";
import { ASR_CAPS, blankFor, LLM_CAPS } from "./providerForm";

export type Role = "asr" | "llm";

export const runtimeFor = (role: Role): Runtime => (role === "asr" ? "whisper" : "llama");

/**
 * A blank network service whose capabilities match the role. Nothing else is made by hand:
 * each installed model and each platform service brings its own engine.
 */
export function blankHttp(role: Role): ProviderConfig {
  const p = blankFor("http");
  p.capabilities = { ...(role === "asr" ? ASR_CAPS : LLM_CAPS) };
  return p;
}

/** Whether the provider can transcribe or rewrite at all. */
export const hasRole = (p: ProviderConfig): boolean => p.capabilities.transcribe || p.capabilities.chat;

/**
 * Providers that can serve the role: speech recognition transcribes, text rewriting chats.
 * Providers with neither capability are listed under speech recognition so they stay editable.
 */
export function enginesOf(config: Config | null, role: Role): ProviderConfig[] {
  return (config?.providers ?? []).filter((p) => (role === "asr" ? p.capabilities.transcribe || !hasRole(p) : p.capabilities.chat));
}

/** A provider id from free text, suffixed with -2, -3, ... while it is taken. */
export function freshId(text: string, existingIds: string[]): string {
  const base = text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "engine";
  let id = base;
  for (let n = 2; existingIds.includes(id); n++) id = `${base}-${n}`;
  return id;
}

/** Recommended catalogue entries of either role that are not installed. */
export function recommendedMissing(manifest: ManifestView | null, installed: InstalledModel[]): ModelEntry[] {
  return (manifest?.models ?? []).filter((m) => m.recommended && !installed.some((i) => i.id === m.id));
}
