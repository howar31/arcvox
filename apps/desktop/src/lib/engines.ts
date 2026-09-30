import type { Config, InstalledModel, ManifestView, ModelEntry, ProviderConfig, ProviderKind, Runtime } from "./api";
import { ASR_CAPS, baseName, blankFor, capsForNative, capsForRuntime, LLM_CAPS, NATIVE_DICTATION, NATIVE_FM } from "./providerForm";

export type Role = "asr" | "llm";

export const runtimeFor = (role: Role): Runtime => (role === "asr" ? "whisper" : "llama");

/** A blank provider of the kind whose runtime, system service and capabilities match the role. */
export function blankForRole(kind: ProviderKind, role: Role): ProviderConfig {
  const p = blankFor(kind);
  if (kind === "embedded") {
    p.runtime = runtimeFor(role);
    p.capabilities = capsForRuntime(p.runtime);
  } else if (kind === "native") {
    p.native_id = role === "llm" ? NATIVE_FM : NATIVE_DICTATION;
    p.capabilities = capsForNative(p.native_id);
  } else {
    p.capabilities = { ...(role === "asr" ? ASR_CAPS : LLM_CAPS) };
  }
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

/** Installed files of the role's runtime that no embedded provider loads, by catalogue id or by path. */
export function unusedModels(installed: InstalledModel[], providers: ProviderConfig[], role: Role): InstalledModel[] {
  const embedded = providers.filter((p) => p.kind === "embedded");
  return installed.filter(
    (m) => m.runtime === runtimeFor(role) && !embedded.some((p) => (p.model_id !== null && p.model_id === m.id) || (p.model_path !== null && p.model_path === m.path)),
  );
}

/** A provider id from free text, suffixed with -2, -3, ... while it is taken. */
export function freshId(text: string, existingIds: string[]): string {
  const base = text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "engine";
  let id = base;
  for (let n = 2; existingIds.includes(id); n++) id = `${base}-${n}`;
  return id;
}

/** An embedded provider that loads the model; the id comes from the model id or file name and is suffixed when taken. */
export function engineFromModel(model: InstalledModel, label: string, existingIds: string[]): ProviderConfig {
  return {
    ...blankFor("embedded"),
    id: freshId(model.custom ? baseName(model.path).replace(/\.[^.]+$/, "") : model.id, existingIds),
    label,
    runtime: model.runtime,
    capabilities: capsForRuntime(model.runtime),
    model_id: model.custom ? null : model.id,
    model_path: model.custom ? model.path : null,
  };
}

/** Recommended catalogue entries of either role that are not installed. */
export function recommendedMissing(manifest: ManifestView | null, installed: InstalledModel[]): ModelEntry[] {
  return (manifest?.models ?? []).filter((m) => m.recommended && !installed.some((i) => i.id === m.id));
}
