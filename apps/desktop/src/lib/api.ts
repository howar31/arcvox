import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Types match the JSON shapes produced by the core crate's serde derives.
export type ProviderKind = "http" | "embedded" | "native";
export interface Capabilities { transcribe: boolean; chat: boolean; streaming: boolean; language_hint: boolean; hotwords: boolean }
export interface ProviderConfig {
  id: string; label: string; kind: ProviderKind; capabilities: Capabilities;
  base_url: string | null; model: string | null; api_key_ref: string | null;
  extra_headers: Record<string, string>; model_path: string | null; native_id: string | null;
}
export type StepSpec = { step: "asr" } | { step: "opencc"; config: string } | { step: "llm" } | { step: "rules" };
export interface Example { raw: string; refined: string }
export interface Matcher { app_ids: string[]; url_patterns: string[] }
export interface Profile {
  id: string; name: string; is_default: boolean; matcher: Matcher; asr_provider: string; llm_provider: string | null;
  language_hint: string | null; prompt_addition: string; few_shot: Example[]; steps: StepSpec[];
}
export interface DictionaryEntry { term: string; aliases: string[]; asr_hint: boolean }
export interface General {
  history_retention_days: number; keep_recordings: boolean; recording_retention_days: number; context_limit_bytes: number;
  asr_timeout_secs: number; llm_timeout_secs: number; llm_temperature: number; llm_max_tokens: number | null;
}
export interface Config { schema_version: number; general: General; providers: ProviderConfig[]; profiles: Profile[]; dictionary: DictionaryEntry[] }
export interface ProbeResult { ok: boolean; failure: { kind: string; message?: string } | null }
export interface Utterance {
  id: number; ts: number; profile_id: string; app_name: string | null; raw_text: string; refined_text: string; final_text: string;
  degraded: boolean; has_audio: boolean; asr_ms: number; llm_ms: number | null;
}
export type InsertMethod = "paste" | "type" | "ax";
export interface ClientSettings {
  schema_version: number;
  hotkey: { hold_threshold_ms: number; double_tap_window_ms: number };
  microphone: string;
  context: { app_name: boolean; window_title: boolean; url: boolean; selected_text: boolean; clipboard: boolean };
  insertion: { per_app: Record<string, InsertMethod> };
  max_utterance_secs: number;
  ui_language: "system" | "zh-tw" | "en";
}
export interface Permissions { accessibility: boolean }
export interface RunningApp { bundle_id: string; name: string }
export interface SystemInfo { version: string; config_dir: string; data_dir: string; cache_dir: string }
export interface CmdError { message: string }

export const getConfig = () => invoke<Config>("config_get");
export const updateGeneral = (general: General) => invoke<Config>("config_update_general", { general });
export const upsertProvider = (provider: ProviderConfig) => invoke<Config>("providers_upsert", { provider });
export const deleteProvider = (id: string) => invoke<Config>("providers_delete", { id });
export const probeProvider = (provider: ProviderConfig) => invoke<ProbeResult>("providers_probe", { provider });
export const upsertProfile = (profile: Profile) => invoke<Config>("profiles_upsert", { profile });
export const deleteProfile = (id: string) => invoke<Config>("profiles_delete", { id });
export const setDefaultProfile = (id: string) => invoke<Config>("profiles_set_default", { id });
export const setDictionary = (entries: DictionaryEntry[]) => invoke<Config>("dictionary_set", { entries });
export const getPrompt = () => invoke<string>("prompt_get");
export const setPrompt = (text: string) => invoke<void>("prompt_set", { text });
export const restorePrompt = () => invoke<string>("prompt_restore");
export const listHistory = (limit = 200) => invoke<Utterance[]>("history_list", { limit });
export const deleteHistory = (id: number) => invoke<void>("history_delete", { id });
export const clearHistory = () => invoke<void>("history_clear");
export const setSecret = (keyRef: string, value: string) => invoke<void>("secrets_set", { keyRef, value });
export const hasSecret = (keyRef: string) => invoke<boolean>("secrets_has", { keyRef });
export const deleteSecret = (keyRef: string) => invoke<void>("secrets_delete", { keyRef });
export const getClient = () => invoke<ClientSettings>("client_get");
export const updateClient = (settings: ClientSettings) => invoke<ClientSettings>("client_update", { settings });
export const listMicrophones = () => invoke<string[]>("system_list_microphones");
export const permissions = () => invoke<Permissions>("system_permissions");
export const requestAccessibility = () => invoke<boolean>("system_request_accessibility");
export const openPrivacyPane = (pane: "accessibility" | "microphone") => invoke<void>("system_open_privacy_pane", { pane });
export const runningApps = () => invoke<RunningApp[]>("system_running_apps");
export const systemInfo = () => invoke<SystemInfo>("system_info");
export const reveal = (path: string) => invoke<void>("system_reveal", { path });
export const uiLanguage = () => invoke<string>("ui_language");
export const widgetCopy = (text: string) => invoke<void>("widget_copy", { text });
export const openSettingsWindow = (page?: string) => invoke<void>("system_open_settings_window", { page: page ?? null });

export function onConfigChanged(cb: (c: Config) => void): Promise<UnlistenFn> {
  return listen<Config>("config-changed", (e) => cb(e.payload));
}

export type WidgetState = "idle" | "recording" | "locked" | "processing" | "inserting" | "no_accessibility" | "engine_error";
export interface StatePayload { state: WidgetState; language: "zh-TW" | "en" }
export interface LevelPayload { rms: number }
export interface MessagePayload { kind: "info" | "error"; key: string; detail: string | null; copyable: string | null; language: "zh-TW" | "en" }
export function onWidgetState(cb: (p: StatePayload) => void): Promise<UnlistenFn> { return listen<StatePayload>("widget-state", (e) => cb(e.payload)); }
export function onWidgetLevel(cb: (p: LevelPayload) => void): Promise<UnlistenFn> { return listen<LevelPayload>("widget-level", (e) => cb(e.payload)); }
export function onWidgetMessage(cb: (p: MessagePayload) => void): Promise<UnlistenFn> { return listen<MessagePayload>("widget-message", (e) => cb(e.payload)); }

export function errorMessage(e: unknown): string {
  if (typeof e === "object" && e && "message" in e) return String((e as CmdError).message);
  return String(e);
}
