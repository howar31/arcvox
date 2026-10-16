import type { InsertMethod, InstalledModel, ManifestView, Profile, ProviderConfig } from "./api";
import { ownModelDescription } from "./engines";
import { scriptAt, scriptLabel } from "./flowEdit";
import { t } from "./i18n";
import { modelDescription } from "./labels";
import { engineMeta } from "./profileEdit";
import { baseName, NATIVE_FM } from "./providerForm";

export interface StageLine { name: string; meta: string; known: boolean; missing: boolean }

/** Which models are installed; `ids` is null while that is not known yet. */
/** The model files that can run, by id and path; `deaf` holds the ids and paths of speech models present without their audio encoder. */
export interface Installed { ids: Set<string> | null; paths: Set<string>; deaf?: Set<string> }

/** The capture stage: the microphone in use and where it is. */
export function micLine(mic: string, available: string[]): StageLine {
  const name = mic === "builtin" ? t("home.mic.builtin_name") : mic === "default" ? t("general.mic_default") : mic;
  const absent = mic !== "builtin" && mic !== "default" && !available.includes(mic);
  const meta = mic === "builtin" ? t("home.mic.builtin") : mic === "default" ? t("home.mic.default") : absent ? t("home.mic.absent") : t("home.mic.device");
  return { name, meta, known: true, missing: absent };
}

/** Why a local model cannot run: its file is not downloaded, or its audio encoder is missing. */
export type EngineReason = "download" | "encoder";

/** An engine's kind and readiness in words; `reason` says why it cannot run. */
export function engineKind(p: ProviderConfig, installed: Installed): { meta: string; missing: boolean; reason?: EngineReason } {
  const m = engineMeta(p, installed.ids ?? new Set(), installed.ids ? installed.paths : undefined);
  if (m.kind === "builtin") return { meta: t("home.meta.builtin"), missing: false };
  if (m.kind === "external") return { meta: m.detail ?? t("home.meta.external"), missing: false };
  if (installed.ids === null) return { meta: t("home.meta.local"), missing: false };
  if (!m.ready && (installed.deaf?.has(p.id) || (p.model_path && installed.deaf?.has(p.model_path)))) return { meta: t("home.meta.local_no_encoder"), missing: true, reason: "encoder" };
  return m.ready ? { meta: t("home.meta.local_ready"), missing: false } : { meta: t("home.meta.local_missing"), missing: true, reason: "download" };
}

/** The engines to choose from, in the Engines page's groups; in each group the ones that can run come first. */
export function engineGroups(engines: ProviderConfig[], installed: Installed): { kind: ProviderConfig["kind"]; engines: ProviderConfig[] }[] {
  return (["embedded", "native", "http"] as const)
    .map((kind) => {
      const ofKind = engines.filter((p) => p.kind === kind);
      return { kind, engines: [...ofKind.filter((p) => !engineKind(p, installed).missing), ...ofKind.filter((p) => engineKind(p, installed).missing)] };
    })
    .filter((g) => g.engines.length > 0);
}

/** A stage served by an engine; a null id is the rewrite stage turned off. */
export function engineLine(providers: ProviderConfig[], id: string | null, installed: Installed): StageLine {
  if (id === null) return { name: t("home.no_rewrite"), meta: t("home.no_rewrite_meta"), known: true, missing: false };
  const p = providers.find((x) => x.id === id);
  if (!p) return { name: id, meta: t("home.not_found"), known: false, missing: false };
  return { name: p.label, known: true, ...engineKind(p, installed) };
}

/** The output stage; `method` is the default insertion method. */
export function outputLine(profile: Profile, method: InsertMethod = "paste"): StageLine {
  const script = scriptAt(profile, 1);
  const meta = script ? scriptLabel(script.config) : t("home.output_plain");
  return { name: t("home.output_" + method), meta, known: true, missing: false };
}

/** One sentence about what an engine is. */
export function engineDescription(p: ProviderConfig, manifest: ManifestView | null, models: InstalledModel[] = []): string {
  if (p.kind === "native") return t(p.native_id === NATIVE_FM ? "engines.desc.apple_foundation_models" : "engines.desc.apple_dictation");
  if (p.kind === "http") {
    const host = engineMeta(p, new Set()).detail ?? "";
    return p.model ? t("engines.desc.http", { host, model: p.model }) : host;
  }
  if (p.model_id) {
    const entry = manifest?.models.find((m) => m.id === p.model_id);
    return (entry && modelDescription(entry)) || p.model_id;
  }
  if (p.model_path) {
    // A file of the user's own: what its structure says, else its name.
    const own = models.find((m) => m.path === p.model_path);
    return (own && ownModelDescription(own)) || t("engines.desc.custom", { file: baseName(p.model_path) });
  }
  return t("engines.desc.no_model");
}
