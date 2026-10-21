import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { PreviewRequest, Scene } from "./widgetPreview";
import { failureText } from "./failure";
import { has, t } from "./i18n";

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
export type StepSpec = { step: "asr" } | { step: "opencc"; config: string; punctuation?: boolean } | { step: "llm" } | { step: "rules"; dictionary?: boolean; trailing_stop?: boolean; question_mark?: boolean; question_mark_inside?: boolean; spacing?: boolean; punctuation_width?: boolean };
export interface Example { raw: string; refined: string }
export interface Matcher { app_ids: string[]; url_patterns: string[] }
export interface Profile {
  id: string; name: string; is_default: boolean; matcher: Matcher;
  /** The engines the profile names; null follows the default. Whether a rewrite runs is the steps' business. */
  asr_provider: string | null; llm_provider: string | null;
  language_hint: string | null; prompt_addition: string; few_shot: Example[]; steps: StepSpec[];
  /** The built-in preset the profile was made from; a reset returns to it. */
  preset: string | null;
  /** The revision of the template's writing style the profile last took; below `PresetView.revision` the template has moved on since. */
  preset_revision: number;
  /** The dictionary's hinted terms are given to the recognition engine. */
  asr_dictionary_hint: boolean;
  /** What the rewrite model is told besides the utterance, and whether a much shorter rewrite is refused. */
  llm_dictionary_terms: boolean; llm_window_title: boolean; llm_short_fallback: boolean;
}
export interface DictionaryEntry {
  term: string; aliases: string[]; asr_hint: boolean;
  /** Where the entry came from and what it was heard as; absent on entries from before this was kept. */
  origin?: "manual" | "suggestion"; heard?: string[];
  /** Unix seconds when the entry was added and last changed; absent on entries from before this was kept. */
  added?: number; updated?: number;
}
/** A suggestion the user turned down. */
export interface Dismissed { term: string; heard?: string[]; at?: number }
/** The sort keys of terms: stroke count and Bopomofo / pinyin initials of the first Han character, null for other terms. */
export interface TermIndex { strokes: number | null; zhuyin: string | null; pinyin: string | null }
export const dictionaryIndex = (terms: string[]) => invoke<TermIndex[]>("dictionary_index", { terms });
/** How often a term's aliases were replaced, and when last (unix seconds); terms never replaced are absent. */
export interface TermHits { term: string; count: number; last: number }
export const dictionaryHits = () => invoke<TermHits[]>("dictionary_hits");
/** Up to `limit` dictations holding the term, as excerpts with the term between 【 and 】. */
export const dictionaryExamples = (term: string, limit = 2) => invoke<string[]>("dictionary_examples", { term, limit });
export interface General {
  keep_history: boolean; history_retention_days: number; keep_stages: boolean; stage_retention_days: number; keep_recordings: boolean; recording_retention_days: number; keep_stats: boolean; context_limit_bytes: number;
  asr_timeout_secs: number; llm_timeout_secs: number; llm_temperature: number; llm_max_tokens: number | null;
  llm_idle_unload_secs: number; llm_preload_on_record: boolean; retry_count: number; typing_chars_per_min: number;
  /** The folder recordings are kept in; null for the default one. Changed with `setRecordingsDir` only. */
  recordings_dir: string | null;
}
/** The pair a profile runs with when it names no engine, or when the one it names cannot run. */
export interface DefaultEngines { asr: string; llm: string }
export type EngineRole = "asr" | "llm";
export interface Config {
  schema_version: number; general: General; providers: ProviderConfig[]; profiles: Profile[]; dictionary: DictionaryEntry[];
  default_engines: DefaultEngines;
  /** Suggested terms the user turned down. */
  dictionary_dismissed: Dismissed[];
}
/** A dictionary candidate from the dictations the user corrected. */
export interface Suggestion {
  term: string; aliases: { text: string; rows: number }[]; rows: number; excerpts: string[];
  known_term: boolean; near_term: string | null; add_aliases: string[]; form_only: boolean;
  /** Every heard text is a Chinese word that sounds like the term. */
  sound_alike: boolean;
}
export interface ProbeResult { ok: boolean; failure: { kind: string; message?: string; reason?: string; status?: number } | null }
export interface Utterance {
  id: number; ts: number; profile_id: string; app_name: string | null; raw_text: string; refined_text: string; final_text: string;
  degraded: boolean; has_audio: boolean; asr_ms: number; llm_ms: number | null;
  /** Length and size of the recording, when one is kept. */
  audio_ms: number | null; audio_bytes: number | null;
  /** Why the dictation failed; such a row has no text yet. */
  error: string | null;
  /** The kind of failure, when it was kept. */
  error_code: string | null;
  /** How many times the recording was processed again. */
  redone: number;
}
/** What processing a row again gave: the result recorded with the row, or why the engines failed (recorded too). */
export interface RedoOutcome { attempt: Attempt | null; failure: { code: string; detail: string } | null }
/** One result a row had: what processing its recording gave, or how it failed. The chosen one is what the row shows. */
export interface Attempt {
  id: number; ts: number; profile_id: string; asr_label: string | null; llm_label: string | null; raw_text: string; final_text: string;
  degraded: boolean; asr_ms: number; llm_ms: number | null; error: string | null; error_code: string | null;
  /** The result the row had before it was first processed again. */
  origin: boolean; chosen: boolean;
  /** The user's correction of this result's text, when there is one. */
  corrected_text: string | null;
}
/** The nine stages of a dictation, in their fixed order (positions 1 to 9; the script conversion takes 4 and 6). */
export type StageKind = "capture" | "trim" | "asr" | "opencc" | "llm" | "rules" | "insert" | "record";
/** What one stage did in one run: the text it handed on (null for the audio stages and a failed step), its time, and its own facts. */
export interface Stage { position: number; kind: StageKind; text: string | null; ms: number | null; detail: Record<string, unknown> }
/** The newest dictation made with a profile and what its stages did. */
export interface LastRun { row: Utterance; stages: Stage[] }
export type InsertMethod = "paste" | "type" | "ax" | "copy";
export interface ClientSettings {
  schema_version: number;
  /** `shortcuts`: the key combinations and mouse buttons that start a recording, each a list of key names. */
  hotkey: { hold_threshold_ms: number; double_tap_window_ms: number; escape_cancels_processing: boolean; shortcuts: string[][] };
  microphone: string;
  context: { app_name: boolean; window_title: boolean; url: boolean; selected_text: boolean; clipboard: boolean };
  insertion: { default: InsertMethod; keep_on_clipboard: boolean; per_app: Record<string, InsertMethod> };
  /** `read_back`: after an insertion the field is read back and the user's changes kept as the final text. */
  learning: { read_back: boolean };
  /** Switches that write more to the log, for looking into the app's workings; all off by default. */
  dev: { log_read_back: boolean };
  max_utterance_secs: number;
  auto_stop: { enabled: boolean; silence_secs: number };
  show_transcript: boolean;
  widget_size: "x_small" | "small" | "medium" | "large" | "x_large";
  /** What the floating widget shows while no dictation goes on. */
  widget_idle: WidgetIdle;
  /** Whether and how a resting widget turns faint. */
  widget_fade: WidgetFade;
  /** The display the widget stays on; null shows it on the display under the pointer. */
  widget_display: string | null;
  ui_language: "system" | "zh-tw" | "en";
  manifest_url: string | null;
  /** The statistics page's cards: position and size on its grid; null is the built-in arrangement. */
  stats_layout: StatsCard[] | null;
  /** The folder data is backed up to, what goes there, and whether that happens by itself. */
  backup: { folder: string | null; categories: BackupCategories; auto: boolean; recreate: boolean };
}
/** A resting widget turns faint after `after_secs`, to `transparency` percent. */
export interface WidgetFade { enabled: boolean; after_secs: number; transparency: number }
export interface StatsCard { id: string; row: number; col: number; w: number; h: number; hidden: boolean }
export type MicAccess = "granted" | "not_asked" | "denied" | "restricted";
export interface Permissions {
  accessibility: boolean;
  /** The system lets the app watch the keyboard but not act on it. */
  keyboard_watch_only: boolean;
  microphone: MicAccess;
}
export interface RunningApp { bundle_id: string; name: string }
export interface SystemInfo { name: string; version: string; dev: boolean; config_dir: string; data_dir: string; cache_dir: string; recordings_dir: string }
/** A failed command: `code` names the kind of failure for the interface's wording, with `params` for its placeholders; `message` is the reason as the backend gives it. */
export interface CmdError { message: string; code?: string | null; params?: Record<string, string> }

export const getConfig = () => invoke<Config>("config_get");
export const updateGeneral = (general: General) => invoke<Config>("config_update_general", { general });
export const upsertProvider = (provider: ProviderConfig) => invoke<Config>("providers_upsert", { provider });
export const deleteProvider = (id: string) => invoke<Config>("providers_delete", { id });
export const setDefaultEngine = (role: EngineRole, id: string) => invoke<Config>("default_engine_set", { role, id });
export const probeProvider = (provider: ProviderConfig) => invoke<ProbeResult>("providers_probe", { provider });
export const upsertProfile = (profile: Profile) => invoke<Config>("profiles_upsert", { profile });
export const deleteProfile = (id: string) => invoke<Config>("profiles_delete", { id });
export const setDefaultProfile = (id: string) => invoke<Config>("profiles_set_default", { id });
/** Resets to the profile's own preset, or to `preset` when given (the standard one, when its own is gone). */
export const resetProfile = (id: string, preset?: string) => invoke<Config>("profiles_reset", { id, preset: preset ?? null });
export const setDictionary = (entries: DictionaryEntry[], dismissed: Dismissed[]) => invoke<Config>("dictionary_set", { entries, dismissed });
export const dictionarySuggestions = () => invoke<Suggestion[]>("dictionary_suggestions");
export const getPrompt = () => invoke<string>("prompt_get");
export const setPrompt = (text: string) => invoke<void>("prompt_set", { text });
export const defaultPrompt = () => invoke<string>("prompt_default");
export const restorePrompt = () => invoke<string>("prompt_restore");
/** Recent rows; `query` keeps rows whose text contains it, `withAudio` rows with a recording. */
/** One page of history rows, newest first. `since` and `until` are unix seconds (end exclusive); `before` is the last row shown. */
export interface HistoryPage { limit: number; query?: string; withAudio?: boolean; since?: number | null; until?: number | null; before?: { ts: number; id: number } | null }
export const listHistory = (p: HistoryPage) =>
  invoke<Utterance[]>("history_list", { limit: p.limit, query: p.query ?? "", withAudio: p.withAudio ?? false, since: p.since ?? null, until: p.until ?? null, beforeTs: p.before?.ts ?? null, beforeId: p.before?.id ?? null });
export const historyAudio = (id: number) => invoke<ArrayBuffer>("history_audio", { id });
export const reprocessHistory = (id: number, profileId: string | null) => invoke<RedoOutcome>("history_reprocess", { id, profileId });
/** Every result a row had, newest first. */
export const historyAttempts = (id: number) => invoke<Attempt[]>("history_attempts", { id });
export const historyStages = (id: number) => invoke<Stage[]>("history_stages", { id });
export const historyLast = (profileId: string) => invoke<LastRun | null>("history_last", { profileId });
export const chooseAttempt = (id: number, attempt: number) => invoke<void>("history_choose_attempt", { id, attempt });
export const deleteAttempt = (attempt: number) => invoke<void>("history_delete_attempt", { attempt });
export const deleteRecording = (id: number) => invoke<void>("history_delete_recording", { id });
/** Deletes every recording and returns how many files were removed. */
export const clearRecordings = () => invoke<number>("history_clear_recordings");
export const revealRecording = (id: number) => invoke<void>("history_reveal_recording", { id });
export interface HistoryStorage {
  rows: number; recordings: number; recording_bytes: number; text_bytes: number; stage_bytes: number;
  /** Stage records and rerun results of the rows; the statistics buckets and the database size are of the whole database. */
  stages: number; attempts: number; stats_hours: number; db_bytes: number;
}
/** Drops every stage record; returns how many went. */
export const clearStages = () => invoke<number>("history_clear_stages");
/** Drops every rerun result; returns how many went. */
export const clearAttempts = () => invoke<number>("history_clear_attempts");
/** What the rows take: all of them, or those from `since` up to `until` (unix seconds). */
export const historyStorage = (since: number | null = null, until: number | null = null) => invoke<HistoryStorage>("history_storage", { since, until });
/** Removes the rows in the range with their recordings; returns how many rows went. */
export const clearHistoryRange = (since: number | null, until: number | null) => invoke<number>("history_clear_range", { since, until });
/** Removes the recordings of the rows in the range; returns how many files went. */
export const clearRecordingsRange = (since: number | null, until: number | null) => invoke<number>("history_clear_recordings_range", { since, until });
/** Rows per local day (`day` is YYYY-MM-DD), days without rows left out. */
export interface DayCount { day: string; count: number }
export const historyDays = (tzOffsetSecs: number, withAudio = false) => invoke<DayCount[]>("history_days", { tzOffsetSecs, withAudio });
/** Rows per local hour (24 counts) between `since` and `until`, unix seconds. */
export const historyHours = (since: number, until: number, tzOffsetSecs: number, withAudio = false) => invoke<number[]>("history_hours", { since, until, tzOffsetSecs, withAudio });
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
export interface StatsDay { day: number; count: number; chars: number; audio_ms: number }
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
/** The settings of a new installation. */
export interface Defaults { client: ClientSettings; general: General }
export const getDefaults = () => invoke<Defaults>("settings_defaults");
export const updateClient = (settings: ClientSettings) => invoke<ClientSettings>("client_update", { settings });
export const listMicrophones = () => invoke<string[]>("system_list_microphones");
/** A connected display: the key the setting stores, and the name the system shows for it. */
export interface DisplayName { key: string; name: string }
export const listDisplays = () => invoke<DisplayName[]>("system_list_displays");
export const permissions = () => invoke<Permissions>("system_permissions");
export const requestAccessibility = () => invoke<boolean>("system_request_accessibility");
export const requestMicrophone = () => invoke<MicAccess>("system_request_microphone");
/** The settings pages that have a problem to show. */
/** A settings page with a problem, at its worst problem's level: dictation fails, or it goes on and asks for attention. */
export interface PageProblem { page: string; level: "fails" | "attention" }
export const problemPages = () => invoke<PageProblem[]>("system_problem_pages");
export const onProblemPages = (cb: (pages: PageProblem[]) => void) => listen<PageProblem[]>("problem-pages", (e) => cb(e.payload));
export const openPrivacyPane = (pane: "accessibility" | "microphone" | "files") => invoke<void>("system_open_privacy_pane", { pane });
export const runningApps = () => invoke<RunningApp[]>("system_running_apps");
export const appNames = (bundleIds: string[]) => invoke<Record<string, string | null>>("system_app_names", { bundleIds });
export const systemInfo = () => invoke<SystemInfo>("system_info");
export const reveal = (path: string) => invoke<void>("system_reveal", { path });
export const openFolder = (path: string) => invoke<void>("system_open_folder", { path });
export type AboutLink = "sponsor" | "source" | "issue" | "website";
export const openLink = (link: AboutLink) => invoke<void>("system_open_link", { link });
export const uiLanguage = () => invoke<string>("ui_language");
export const widgetCopy = (text: string) => invoke<void>("widget_copy", { text });
export const widgetDismiss = () => invoke<void>("widget_dismiss");
export const widgetDragStart = () => invoke<void>("widget_drag_start");
/** The length the widget's labels need in each shape. */
/** The factor of the widget size the user chose. */
export const widgetScale = () => invoke<number>("widget_scale");
export const widgetIdle = () => invoke<WidgetIdle>("widget_idle");
export const widgetFade = () => invoke<WidgetFade>("widget_fade");
/** Opens the menu of the menu-bar icon at the pointer. */
export const widgetMenu = () => invoke<void>("widget_menu");
export const widgetMeasure = (wide: number, tall: number) => invoke<void>("widget_measure", { wide, tall });
/** The size of the panel's content; null when it has none. */
export const panelFit = (width: number | null, height: number | null) => invoke<void>("panel_fit", { width, height });
/** Why `keys` cannot be added to `existing`; null when they can. */
export const shortcutCheck = (keys: string[], existing: string[][]) => invoke<string | null>("shortcut_check", { keys, existing });
/** Starts or ends recording a new shortcut; false when keys cannot be recorded. */
export const shortcutCapture = (on: boolean) => invoke<boolean>("shortcut_capture", { on });
export const pauseSet = (on: boolean) => invoke<void>("pause_set", { on });
export const pauseState = () => invoke<boolean>("pause_state");
export function onDictationPaused(cb: (on: boolean) => void): Promise<UnlistenFn> { return listen<boolean>("dictation-paused", (e) => cb(e.payload)); }
/** The keys held while a shortcut is recorded; `done` when the combination was let go, `cancelled` when recording ended without one. */
export interface CapturePayload { keys: string[]; done: boolean; cancelled: boolean }
export function onShortcutCapture(cb: (p: CapturePayload) => void): Promise<UnlistenFn> { return listen<CapturePayload>("shortcut-capture", (e) => cb(e.payload)); }
export const widgetPlacing = (on: boolean) => invoke<void>("widget_placing", { on });
export const widgetIsPlacing = () => invoke<boolean>("widget_is_placing");
/** Shows the widget in a chosen state with settings that are not saved, while it is kept on screen. */
export const widgetPreview = (preview: PreviewRequest) => invoke<void>("widget_preview", { preview });
/** The app changed the state the previewed widget shows: a real dictation began, or the widget is dragged. */
export function onWidgetPreviewScene(cb: (scene: Scene) => void): Promise<UnlistenFn> { return listen<Scene>("widget-preview-scene", (e) => cb(e.payload)); }
/** Nothing of an earlier dictation or preview is shown any more. */
export function onWidgetReset(cb: () => void): Promise<UnlistenFn> { return listen("widget-reset", () => cb()); }
export const widgetResetPosition = () => invoke<void>("widget_reset_position");
export const openSettingsWindow = (page?: string) => invoke<void>("system_open_settings_window", { page: page ?? null });

export function onConfigChanged(cb: (c: Config) => void): Promise<UnlistenFn> {
  return listen<Config>("config-changed", (e) => cb(e.payload));
}

export type WidgetState = "idle" | "recording" | "locked" | "processing" | "inserting" | "placing" | "moving" | "no_accessibility" | "paused" | "engine_error";
/** What the widget shows while no dictation goes on: nothing, the ring alone, or the whole widget with a label. */
export type WidgetIdle = "hidden" | "ring" | "label";
/** `faint`: a resting widget is shown faint without waiting, while its transparency is being chosen. */
export interface StatePayload { state: WidgetState; language: "zh-TW" | "en"; scale?: number; idle?: WidgetIdle; fade?: WidgetFade; faint?: boolean }
export interface LevelPayload { rms: number }
/** `code` is the kind of failure of a failed dictation; `retry` says it was kept for a retry from the history. */
export interface MessagePayload { kind: "info" | "error"; key: string; detail: string | null; copyable: string | null; code: string | null; retry: boolean; language: "zh-TW" | "en" }
export function onWidgetState(cb: (p: StatePayload) => void): Promise<UnlistenFn> { return listen<StatePayload>("widget-state", (e) => cb(e.payload)); }
/** The widget is kept on screen to be positioned, or no longer: switched in the settings window or in a menu. */
export function onWidgetPlacing(cb: (on: boolean) => void): Promise<UnlistenFn> { return listen<boolean>("widget-placing", (e) => cb(e.payload)); }
/** The pointer came over a resting widget (true) or left it. */
export function onWidgetHover(cb: (over: boolean) => void): Promise<UnlistenFn> { return listen<boolean>("widget-hover", (e) => cb(e.payload)); }
/** What the widget shows while idle was chosen, in the settings window or in a menu. */
export function onWidgetIdle(cb: (idle: WidgetIdle) => void): Promise<UnlistenFn> { return listen<WidgetIdle>("widget-idle", (e) => cb(e.payload)); }
/** What the app is doing now, and each change of it, for the settings window. */
export const dictationState = () => invoke<WidgetState>("dictation_state");
export function onDictationState(cb: (state: WidgetState) => void): Promise<UnlistenFn> { return listen<WidgetState>("dictation-state", (e) => cb(e.payload)); }
/** Whether the widget is turned over: its ring at the bottom, opening upwards. */
export function onWidgetFlip(cb: (flipped: boolean) => void): Promise<UnlistenFn> { return listen<boolean>("widget-flip", (e) => cb(e.payload)); }
export function onWidgetLevel(cb: (p: LevelPayload) => void): Promise<UnlistenFn> { return listen<LevelPayload>("widget-level", (e) => cb(e.payload)); }
export function onWidgetMessage(cb: (p: MessagePayload) => void): Promise<UnlistenFn> { return listen<MessagePayload>("widget-message", (e) => cb(e.payload)); }

/** A failed command in the interface language when its code has a text; else the backend's reason. */
export function errorMessage(e: unknown): string {
  if (typeof e === "object" && e && "message" in e) {
    const err = e as CmdError;
    // A dictation that failed again: worded by the failure's own code.
    if (err.code === "dictation_failed") return failureText(err.params?.code ?? null, err.message);
    if (err.code && has("error." + err.code)) return t("error." + err.code, err.params ?? {});
    return String(err.message);
  }
  return String(e);
}

export interface ModelFile { url: string; sha256: string; size_bytes: number }
export interface ModelEntry { id: string; runtime: Runtime; role: "asr" | "llm"; label: string; description: string; languages: string[]; url: string; sha256: string; size_bytes: number; license: string; recommended: boolean; min_ram_gb: number; mmproj?: ModelFile }
export interface ManifestView { source: "remote" | "cached" | "bundled"; models: ModelEntry[] }
/** An installed model: what its file can be used for, and the audio encoder it hears through. */
/** What a model file's structure says about it: architecture and size, quantisation, licence. */
export interface ModelFacts { shape: string | null; quantization: string | null; license: string | null }
export interface InstalledModel { id: string; runtime: Runtime; path: string; size_bytes: number; custom: boolean; capabilities: Capabilities; encoder: string | null; facts: ModelFacts }
export interface DownloadProgress { id: string; received: number; total: number; done: boolean; error: string | null }
export const modelsManifest = () => invoke<ManifestView>("models_manifest");
export const modelsManifestNow = () => invoke<ManifestView>("models_manifest_now");
export const modelsInstalled = () => invoke<InstalledModel[]>("models_installed");
export const modelsDownload = (id: string) => invoke<InstalledModel>("models_download", { id });
export const modelsDownloadRecommended = () => invoke<void>("models_download_recommended");
export const modelsCancel = (id: string) => invoke<void>("models_cancel", { id });
export const modelsDelete = (id: string) => invoke<void>("models_delete", { id });
export const modelsImport = (path: string) => invoke<void>("models_import", { path });
export interface ModelsDir { path: string; is_default: boolean; moved: number; left: number }
export const modelsDirGet = () => invoke<ModelsDir>("models_dir_get");
export const modelsDirSet = (path: string | null) => invoke<ModelsDir>("models_dir_set", { path });
export const modelsDirOpen = () => invoke<void>("models_dir_open");
export const modelsCustomDir = () => invoke<string>("models_custom_dir");
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

/** `suggested_*` are the engines the template was tuned with; null for an untuned template. */
export interface PresetView { id: string; app_ids: string[]; suggested_asr: string | null; suggested_llm: string | null; revision: number }
export interface ClipStatus { present: boolean; secs: number; recorded_at: number | null }
export interface ClipCandidate { id: number; ts: number; raw_text: string }
export type BenchRole = "asr" | "llm";
export interface BenchRun { provider_id: string; role: BenchRole; cold: boolean; load_ms: number; infer_ms: number; output: string; error: string | null }
export const profilesPresets = () => invoke<PresetView[]>("profiles_presets");
export const profilesAddPreset = (presetId: string, name: string) => invoke<Config>("profiles_add_preset", { presetId, name });
/** Settles a profile whose template's writing style moved on: take the template's current style, or keep the profile's own. */
export const profilesTemplateStyle = (id: string, take: boolean) => invoke<Config>("profiles_template_style", { id, take });
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
/** The widget is shown after it was hidden, or is about to be hidden. */
export function onWidgetEnter(cb: () => void): Promise<UnlistenFn> { return listen("widget-enter", () => cb()); }
export function onWidgetLeave(cb: () => void): Promise<UnlistenFn> { return listen("widget-leave", () => cb()); }
export type PanelEdge = "top" | "bottom" | "left" | "right";
/** The panel's window is on screen at its place; the payload is the edge that faces the widget. */
export function onPanelPlaced(cb: (edge: PanelEdge) => void): Promise<UnlistenFn> { return listen<PanelEdge>("panel-placed", (e) => cb(e.payload)); }
export function onWidgetInserted(cb: () => void): Promise<UnlistenFn> { return listen("widget-inserted", () => cb()); }
export function onWidgetCopied(cb: () => void): Promise<UnlistenFn> { return listen("widget-copied", () => cb()); }
export function onWidgetRefined(cb: (p: TranscriptPayload) => void): Promise<UnlistenFn> { return listen<TranscriptPayload>("widget-refined", (e) => cb(e.payload)); }
export function onWidgetTranscript(cb: (p: TranscriptPayload) => void): Promise<UnlistenFn> { return listen<TranscriptPayload>("widget-transcript", (e) => cb(e.payload)); }

// Settings files, backups of the history, and devices.
/** What is backed up to the folder: this device's settings, its content, and its records by kind. */
export interface BackupCategories { settings: boolean; content: boolean; history: boolean; stats: boolean; recordings: boolean }
export interface SettingsInfo { file_name: string; app_version: string; platform: string; device_name: string; exported_at: number; sections: string[]; unknown: string[] }
export type SettingsNote =
  | { kind: "needs_key"; engine: string }
  | { kind: "engine_replaced"; profile: string; role: string }
  | { kind: "default_engine_reset"; role: string }
  | { kind: "engine_unavailable"; engine: string }
  | { kind: "unknown_section"; name: string }
  | { kind: "apps_not_installed"; names: string[] };
export interface SettingsResult { applied: string[]; notes: SettingsNote[] }
export interface BackupSizes { rows: number; recordings: number; recording_bytes: number }
export interface WriteReport { rows: number; cells: number; recordings: number; recording_bytes: number; settings: boolean; recreated: boolean }
export interface BackupDevice {
  id: string; name: string; platform: string; rows: number; first_month: string | null; last_month: string | null;
  recordings: number; recording_bytes: number; has_stats: boolean; has_settings: boolean; this: boolean;
}
export interface BackupInfo { version: number; devices: BackupDevice[] }
export interface ImportReport { devices: number; added: number; updated: number; skipped: number; cells: number; recordings: number; missing_recordings: number; bad_lines: number; expiring: number }
export interface DeviceRow { id: string; name: string; platform: string; rows: number; last_ts: number | null; this: boolean }
export type BackupStatus = { state: "never" } | { state: "ok"; at: number; report: WriteReport } | { state: "failed"; at: number; reason: string; error?: CmdError | null; last_ok: number | null; folder?: FolderState | null } | { state: "running" };
export const exportSettings = (path: string, sections: string[]) => invoke<void>("settings_export", { path, sections });
export const inspectSettings = (path: string) => invoke<SettingsInfo>("settings_inspect", { path });
export const importSettings = (path: string, sections: string[]) => invoke<SettingsResult>("settings_import", { path, sections });
export const backupSizes = () => invoke<BackupSizes>("backup_sizes");
export const exportBackup = (path: string, categories: BackupCategories) => invoke<WriteReport>("backup_export", { path, categories });
export const inspectBackup = (path: string, folder: boolean) => invoke<BackupInfo>("backup_inspect", { path, folder });
export const importBackup = (path: string, folder: boolean, devices: string[], categories: BackupCategories) => invoke<ImportReport>("backup_import", { path, folder, devices, categories });
export const backupStatus = () => invoke<BackupStatus>("backup_status");
/** Writes the backup folder now; the result arrives through `onBackupStatus`. */
export const backupRunNow = () => invoke<void>("backup_run_now");
export const openBackupFolder = () => invoke<void>("backup_folder_open");
/** What the backup folder is right now: there, gone, or refusing a write. */
export type FolderState = "usable" | "missing" | "not_writable" | "blocked";
/** Looks at the backup folder; `probe` writes a small file to find out whether it takes one. */
export const backupFolderState = (probe: boolean) => invoke<FolderState>("backup_folder_state", { probe });
/** Makes the chosen folder again and backs up to it; answers with what the folder is afterwards. */
export const backupFolderCreate = () => invoke<FolderState>("backup_folder_create");
export function onBackupFolder(cb: (s: FolderState) => void): Promise<UnlistenFn> { return listen<FolderState>("backup-folder", (e) => cb(e.payload)); }
export const listDevices = () => invoke<DeviceRow[]>("devices_list");
export const renameDevice = (id: string, name: string) => invoke<void>("device_rename", { id, name });
/** Removes another device's history and statistics from this one; returns how many rows went. */
export const removeDevice = (id: string) => invoke<number>("device_remove", { id });
export function onBackupStatus(cb: (s: BackupStatus) => void): Promise<UnlistenFn> { return listen<BackupStatus>("backup-status", (e) => cb(e.payload)); }
