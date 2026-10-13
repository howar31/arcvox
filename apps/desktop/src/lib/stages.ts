import type { InsertMethod, ManifestView, Profile, ProviderConfig } from "./api";
import { scriptAt, scriptLabel } from "./flowEdit";
import { t } from "./i18n";
import { modelDescription } from "./labels";
import { engineMeta } from "./profileEdit";
import { baseName, NATIVE_FM } from "./providerForm";

export interface StageLine { name: string; meta: string; known: boolean; missing: boolean }

/** Which models are installed; `ids` is null while that is not known yet. */
export interface Installed { ids: Set<string> | null; paths: Set<string> }

/** The capture stage: the microphone in use and where it is. */
export function micLine(mic: string, available: string[]): StageLine {
  const name = mic === "builtin" ? t("home.mic.builtin_name") : mic === "default" ? t("general.mic_default") : mic;
  const absent = mic !== "builtin" && mic !== "default" && !available.includes(mic);
  const meta = mic === "builtin" ? t("home.mic.builtin") : mic === "default" ? t("home.mic.default") : absent ? t("home.mic.absent") : t("home.mic.device");
  return { name, meta, known: true, missing: absent };
}

/** An engine's kind and readiness in words. */
export function engineKind(p: ProviderConfig, installed: Installed): { meta: string; missing: boolean } {
  const m = engineMeta(p, installed.ids ?? new Set(), installed.ids ? installed.paths : undefined);
  if (m.kind === "builtin") return { meta: t("home.meta.builtin"), missing: false };
  if (m.kind === "external") return { meta: m.detail ?? t("home.meta.external"), missing: false };
  if (installed.ids === null) return { meta: t("home.meta.local"), missing: false };
  return { meta: t(m.ready ? "home.meta.local_ready" : "home.meta.local_missing"), missing: !m.ready };
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
export function engineDescription(p: ProviderConfig, manifest: ManifestView | null): string {
  if (p.kind === "native") return t(p.native_id === NATIVE_FM ? "engines.desc.apple_foundation_models" : "engines.desc.apple_dictation");
  if (p.kind === "http") {
    const host = engineMeta(p, new Set()).detail ?? "";
    return p.model ? t("engines.desc.http", { host, model: p.model }) : host;
  }
  if (p.model_id) {
    const entry = manifest?.models.find((m) => m.id === p.model_id);
    return (entry && modelDescription(entry)) || p.model_id;
  }
  if (p.model_path) return t("engines.desc.custom", { file: baseName(p.model_path) });
  return t("engines.desc.no_model");
}
