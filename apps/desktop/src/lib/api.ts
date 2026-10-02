import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Types match the JSON shapes produced by the core crate's serde derives.
export type Runtime = "whisper" | "llama";
export type ProviderKind = "http" | "embedded" | "native";
export interface Capabilities { transcribe: boolean; chat: boolean; streaming: boolean; language_hint: boolean; hotwords: boolean }
export interface ProviderConfig {
  id: string; label: string; kind: ProviderKind; capabilities: Capabilities;
  base_url: string | null; model: string | null; api_key_ref: string | null;
  extra_headers: Record<string, string>; model_path: string | null; native_id: string | null;
  model_id: string | null; runtime: Runtime | null;
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
  llm_idle_unload_secs: number; retry_count: number; typing_chars_per_min: number;
  /** The folder recordings are kept in; null for the default one. Changed with `setRecordingsDir` only. */
  recordings_dir: string | null;
}
export interface Config { schema_version: number; general: General; providers: ProviderConfig[]; profiles: Profile[]; dictionary: DictionaryEntry[] }
export interface ProbeResult { ok: boolean; failure: { kind: string; message?: string; reason?: string } | null }
export interface Utterance {
  id: number; ts: number; profile_id: string; app_name: string | null; raw_text: string; refined_text: string; final_text: string;
  degraded: boolean; has_audio: boolean; asr_ms: number; llm_ms: number | null;
  /** Length and size of the recording, when one is kept. */
  audio_ms: number | null; audio_bytes: number | null;
  /** Why the dictation failed; such a row has no text yet. */
  error: string | null;
  /** The kind of failure, when it was kept. */
  error_code: string | null;
}
/** What processing a row again gave: a result, or why the engines failed. */
export interface RedoOutcome { redo: Redo | null; failure: { code: string; detail: string } | null }
/** The result of processing a row's recording again; `applied` when it was stored at once (the row had failed). */
export interface Redo {
  applied: boolean; profile_id: string; raw_text: string; final_text: string; degraded: boolean; asr_ms: number; llm_ms: number | null;
  dict_fixes: number; snippets: number; llm_changed: boolean; asr_label: string; llm_label: string | null;
}
export type InsertMethod = "paste" | "type" | "ax";
export interface ClientSettings {
  schema_version: number;
  /** `shortcuts`: the key combinations and mouse buttons that start a recording, each a list of key names. */
  hotkey: { hold_threshold_ms: number; double_tap_window_ms: number; shortcuts: string[][] };
  microphone: string;
  context: { app_name: boolean; window_title: boolean; url: boolean; selected_text: boolean; clipboard: boolean };
  insertion: { per_app: Record<string, InsertMethod> };
  max_utterance_secs: number;
  auto_stop: { enabled: boolean; silence_secs: number };
  show_transcript: boolean;
  widget_size: "small" | "medium" | "large";
  ui_language: "system" | "zh-tw" | "en";
  manifest_url: string | null;
}
export interface Permissions { accessibility: boolean }
export interface RunningApp { bundle_id: string; name: string }
export interface SystemInfo { version: string; config_dir: string; data_dir: string; cache_dir: string; recordings_dir: string }
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
/** Recent rows; `query` keeps rows whose text contains it, `withAudio` rows with a recording. */
export const listHistory = (limit = 200, query = "", withAudio = false) => invoke<Utterance[]>("history_list", { limit, query, withAudio });
export const historyAudio = (id: number) => invoke<ArrayBuffer>("history_audio", { id });
export const reprocessHistory = (id: number, profileId: string | null) => invoke<RedoOutcome>("history_reprocess", { id, profileId });
export const applyRedo = (id: number, redo: Redo) => invoke<void>("history_apply_redo", { id, redo });
export const deleteRecording = (id: number) => invoke<void>("history_delete_recording", { id });
/** Deletes every recording and returns how many files were removed. */
export const clearRecordings = () => invoke<number>("history_clear_recordings");
export const revealRecording = (id: number) => invoke<void>("history_reveal_recording", { id });
export interface HistoryStorage { rows: number; recordings: number; recording_bytes: number }
export const historyStorage = () => invoke<HistoryStorage>("history_storage");
/** The folder recordings are kept in, and what the last change of it moved. */
export interface RecordingsDir { path: string; is_default: boolean; moved: number; left: number }
export const recordingsDir = () => invoke<RecordingsDir>("recordings_dir_get");
/** Keeps recordings in `path` from now on (the default folder for null) and moves the existing ones there. */
/** Opens the recordings folder in the file manager. */
export const openRecordingsDir = () => invoke<void>("recordings_dir_open");
export const setRecordingsDir = (path: string | null) => invoke<RecordingsDir>("recordings_dir_set", { path });
export interface StatsTotals {
  count: number; chars: number; audio_ms: number; timed_count: number; timed_chars: number; speed_chars: number; speed_audio_ms: number;
  asr_ms: number; llm_ms: number; llm_count: number; changed: number; degraded: number; unrewritten: number; dict_fixes: number; snippets: number;
  han: number; latin: number; kana: number; max_chars: number; max_audio_ms: number; max_total_ms: number; max_cpm: number;
}
/** `day` counts local days since the epoch. */
export interface StatsDay { day: number; count: number; chars: number }
export interface StatsNamed { name: string; count: number; chars: number }
export interface Stats {
  totals: StatsTotals; saved_ms: number | null; days: StatsDay[]; hours: number[];
  apps: StatsNamed[]; profiles: StatsNamed[]; active_days: StatsDay[];
}
/** Statistics from `since` (seconds since the epoch) on, grouped by the local time `tzOffsetSecs` east of UTC. */
export const getStats = (since: number, tzOffsetSecs: number) => invoke<Stats>("stats_get", { since, tzOffsetSecs });
export const resetStats = () => invoke<void>("stats_reset");
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
export const appNames = (bundleIds: string[]) => invoke<Record<string, string | null>>("system_app_names", { bundleIds });
export const systemInfo = () => invoke<SystemInfo>("system_info");
export const reveal = (path: string) => invoke<void>("system_reveal", { path });
export const uiLanguage = () => invoke<string>("ui_language");
export const widgetCopy = (text: string) => invoke<void>("widget_copy", { text });
export const widgetDismiss = () => invoke<void>("widget_dismiss");
export const widgetDragStart = () => invoke<void>("widget_drag_start");
/** The length the widget's labels need in each shape. */
/** The factor of the widget size the user chose. */
export const widgetScale = () => invoke<number>("widget_scale");
export const widgetMeasure = (wide: number, tall: number) => invoke<void>("widget_measure", { wide, tall });
/** The size of the panel's content; null when it has none. */
export const panelFit = (width: number | null, height: number | null) => invoke<void>("panel_fit", { width, height });
/** Why `keys` cannot be added to `existing`; null when they can. */
export const shortcutCheck = (keys: string[], existing: string[][]) => invoke<string | null>("shortcut_check", { keys, existing });
/** Starts or ends recording a new shortcut; false when keys cannot be recorded. */
export const shortcutCapture = (on: boolean) => invoke<boolean>("shortcut_capture", { on });
/** The keys held while a shortcut is recorded; `done` when the combination was let go, `cancelled` when recording ended without one. */
export interface CapturePayload { keys: string[]; done: boolean; cancelled: boolean }
export function onShortcutCapture(cb: (p: CapturePayload) => void): Promise<UnlistenFn> { return listen<CapturePayload>("shortcut-capture", (e) => cb(e.payload)); }
export const widgetPlacing = (on: boolean) => invoke<void>("widget_placing", { on });
export const widgetResetPosition = () => invoke<void>("widget_reset_position");
export const openSettingsWindow = (page?: string) => invoke<void>("system_open_settings_window", { page: page ?? null });

export function onConfigChanged(cb: (c: Config) => void): Promise<UnlistenFn> {
  return listen<Config>("config-changed", (e) => cb(e.payload));
}

export type WidgetState = "idle" | "recording" | "locked" | "processing" | "inserting" | "placing" | "no_accessibility" | "engine_error";
export interface StatePayload { state: WidgetState; language: "zh-TW" | "en"; scale?: number }
export interface LevelPayload { rms: number }
/** `code` is the kind of failure of a failed dictation; `retry` says it was kept for a retry from the history. */
export interface MessagePayload { kind: "info" | "error"; key: string; detail: string | null; copyable: string | null; code: string | null; retry: boolean; language: "zh-TW" | "en" }
export function onWidgetState(cb: (p: StatePayload) => void): Promise<UnlistenFn> { return listen<StatePayload>("widget-state", (e) => cb(e.payload)); }
export function onWidgetLevel(cb: (p: LevelPayload) => void): Promise<UnlistenFn> { return listen<LevelPayload>("widget-level", (e) => cb(e.payload)); }
export function onWidgetMessage(cb: (p: MessagePayload) => void): Promise<UnlistenFn> { return listen<MessagePayload>("widget-message", (e) => cb(e.payload)); }

export function errorMessage(e: unknown): string {
  if (typeof e === "object" && e && "message" in e) return String((e as CmdError).message);
  return String(e);
}

export interface ModelEntry { id: string; runtime: Runtime; role: "asr" | "llm"; label: string; description: string; languages: string[]; url: string; sha256: string; size_bytes: number; license: string; recommended: boolean; min_ram_gb: number }
export interface ManifestView { source: "remote" | "cached" | "bundled"; models: ModelEntry[] }
export interface InstalledModel { id: string; runtime: Runtime; path: string; size_bytes: number; custom: boolean }
export interface DownloadProgress { id: string; received: number; total: number; done: boolean; error: string | null }
export const modelsManifest = () => invoke<ManifestView>("models_manifest");
export const modelsManifestNow = () => invoke<ManifestView>("models_manifest_now");
export const modelsInstalled = () => invoke<InstalledModel[]>("models_installed");
export const modelsDownload = (id: string) => invoke<InstalledModel>("models_download", { id });
export const modelsDownloadRecommended = () => invoke<void>("models_download_recommended");
export const modelsCancel = (id: string) => invoke<void>("models_cancel", { id });
export const modelsDelete = (id: string) => invoke<void>("models_delete", { id });
export const modelsImport = (path: string) => invoke<InstalledModel>("models_import", { path });
export const modelsDir = () => invoke<string>("models_dir");
export function onModelDownloadProgress(cb: (p: DownloadProgress) => void): Promise<UnlistenFn> {
  return listen<DownloadProgress>("model-download-progress", (e) => cb(e.payload));
}
export function formatBytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(2)} GB`;
  if (n >= 1e7) return `${Math.round(n / 1e6)} MB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)} MB`;
  return `${Math.round(n / 1e3)} KB`;
}
export type NativeStatusKind = "ready" | "needs_download" | "locale_unsupported" | "os_too_old" | "not_eligible" | "intelligence_off" | "model_not_ready" | "failed";
export interface NativeStatusView { status: NativeStatusKind; reason: string | null }
export const nativeStatus = (nativeId: string, hint: string | null) => invoke<NativeStatusView>("native_status", { nativeId, languageHint: hint });
export const nativeInstallLanguage = (hint: string | null) => invoke<void>("native_install_language", { languageHint: hint });
export function onNativeInstallProgress(cb: (fraction: number) => void): Promise<UnlistenFn> {
  return listen<{ fraction: number }>("native-install-progress", (e) => cb(e.payload.fraction));
}

export interface PresetView { id: string; app_ids: string[] }
export interface ClipStatus { present: boolean; secs: number; recorded_at: number | null }
export interface ClipCandidate { id: number; ts: number; raw_text: string }
export type BenchRole = "asr" | "llm";
export interface BenchRun { provider_id: string; role: BenchRole; cold: boolean; load_ms: number; infer_ms: number; output: string; error: string | null }
export const profilesPresets = () => invoke<PresetView[]>("profiles_presets");
export const profilesAddPreset = (presetId: string, name: string) => invoke<Config>("profiles_add_preset", { presetId, name });
export const benchClipStatus = () => invoke<ClipStatus>("bench_clip_status");
export const benchRecordStart = () => invoke<void>("bench_record_start");
export const benchRecordStop = () => invoke<ClipStatus>("bench_record_stop");
export const benchClipFromHistory = (id: number) => invoke<ClipStatus>("bench_clip_from_history", { id });
export const benchHistoryCandidates = () => invoke<ClipCandidate[]>("bench_history_candidates");
export const benchStart = (asr: string[], llm: string[], fallbackText: string) => invoke<void>("bench_start", { asr, llm, fallbackText });
export const benchCancel = () => invoke<void>("bench_cancel");
export function onBenchLevel(cb: (rms: number) => void): Promise<UnlistenFn> {
  return listen<{ rms: number }>("bench-level", (e) => cb(e.payload.rms));
}
export function onBenchProgress(cb: (p: { run: BenchRun; remaining: number }) => void): Promise<UnlistenFn> {
  return listen<{ run: BenchRun; remaining: number }>("bench-progress", (e) => cb(e.payload));
}
export function onBenchDone(cb: () => void): Promise<UnlistenFn> {
  return listen<Record<string, never>>("bench-done", () => cb());
}
export interface TranscriptPayload { text: string; language: "zh-TW" | "en" }
/** The final text of a dictation, sent when the processing is over. */
/** The text was put into the application. */
export function onHistoryChanged(cb: () => void): Promise<UnlistenFn> { return listen("history-changed", () => cb()); }
export function onWidgetInserted(cb: () => void): Promise<UnlistenFn> { return listen("widget-inserted", () => cb()); }
export function onWidgetRefined(cb: (p: TranscriptPayload) => void): Promise<UnlistenFn> { return listen<TranscriptPayload>("widget-refined", (e) => cb(e.payload)); }
export function onWidgetTranscript(cb: (p: TranscriptPayload) => void): Promise<UnlistenFn> { return listen<TranscriptPayload>("widget-transcript", (e) => cb(e.payload)); }
