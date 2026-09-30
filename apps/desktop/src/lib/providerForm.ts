import type { Capabilities, ProbeResult, ProviderConfig, ProviderKind, Runtime } from "./api";

export const ASR_CAPS: Capabilities = { transcribe: true, chat: false, streaming: false, language_hint: true, hotwords: true };
export const LLM_CAPS: Capabilities = { transcribe: false, chat: true, streaming: false, language_hint: false, hotwords: false };

export const NATIVE_DICTATION = "apple.dictation";
export const NATIVE_FM = "apple.foundation_models";

export const capsForRuntime = (r: Runtime): Capabilities => ({ ...(r === "whisper" ? ASR_CAPS : LLM_CAPS) });
export const capsForNative = (id: string): Capabilities => ({ ...(id === NATIVE_FM ? LLM_CAPS : ASR_CAPS) });

export function blankFor(kind: ProviderKind): ProviderConfig {
  const base: ProviderConfig = {
    id: "", label: "", kind, capabilities: { transcribe: false, chat: true, streaming: false, language_hint: false, hotwords: false },
    base_url: "http://localhost:11434/v1", model: "", api_key_ref: null, extra_headers: {}, model_path: null, native_id: null, model_id: null, runtime: null,
  };
  if (kind === "embedded") {
    return { ...base, capabilities: capsForRuntime("whisper"), base_url: null, model: null, runtime: "whisper" };
  }
  if (kind === "native") {
    return { ...base, capabilities: capsForNative(NATIVE_DICTATION), base_url: null, model: null, native_id: NATIVE_DICTATION };
  }
  return base;
}

export function describeProbe(r: ProbeResult, t: (k: string) => string): string {
  if (r.ok) return t("engines.probe_ok");
  const f = r.failure;
  const extra = f?.reason ?? f?.message ?? "";
  return `${t("engines.probe_failed")}: ${t("engines.failure." + (f?.kind ?? "other"))}${extra ? " (" + extra + ")" : ""}`;
}

/** File name of a model path, for table display. */
export const baseName = (p: string | null): string => (p ? p.split(/[\\/]/).pop() ?? p : "");
