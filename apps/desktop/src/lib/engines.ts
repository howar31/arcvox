import type { Config, InstalledModel, ManifestView, ModelEntry, ProviderConfig } from "./api";
import { t } from "./i18n";
import { ASR_CAPS, blankFor, LLM_CAPS } from "./providerForm";

export type Role = "asr" | "llm";

/** Whether an installed model can serve the role; a model that both transcribes and chats serves both. */
export const modelServes = (m: InstalledModel, role: Role): boolean => (role === "asr" ? m.capabilities.transcribe : m.capabilities.chat);

/** A llama.cpp model that transcribes through an audio encoder it does not have beside it. */
export const encoderMissing = (m: InstalledModel): boolean => m.runtime === "llama" && m.capabilities.transcribe && !m.encoder;

/** The installed models that can run: a model without the audio encoder it needs cannot. */
export const runnable = (models: InstalledModel[]): InstalledModel[] => models.filter((m) => !encoderMissing(m));

/** Bytes of every file a catalogue model needs. */
export const entryBytes = (e: ModelEntry): number => e.size_bytes + (e.mmproj?.size_bytes ?? 0);

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

/** The facts a model file's structure gives, in one line; empty when it gives none. */
export function factsLine(m: InstalledModel): string {
  return [m.facts.shape, m.facts.quantization, m.facts.license].filter((x): x is string => !!x).join(t("engines.fact_sep"));
}

/** A model file of the user's own as a description: its facts, and both uses when it has them. */
export function ownModelDescription(m: InstalledModel): string {
  const facts = factsLine(m);
  const both = m.capabilities.transcribe && m.capabilities.chat ? t("engines.both_roles") : "";
  return [facts, both].filter(Boolean).join(t("engines.fact_sep"));
}
