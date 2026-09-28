import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type WidgetState = "idle" | "recording" | "locked" | "processing" | "inserting" | "no_accessibility" | "engine_error";
export interface StatePayload { state: WidgetState; language: "zh-TW" | "en" }
export interface LevelPayload { rms: number }
export interface MessagePayload { kind: "info" | "error"; key: string; detail: string | null; copyable: string | null; language: "zh-TW" | "en" }

export const uiLanguage = () => invoke<string>("ui_language");
export const widgetCopy = (text: string) => invoke<void>("widget_copy", { text });
export const openSettingsWindow = (page?: string) => invoke<void>("system_open_settings_window", { page: page ?? null });

export function onWidgetState(cb: (p: StatePayload) => void): Promise<UnlistenFn> {
  return listen<StatePayload>("widget-state", (e) => cb(e.payload));
}
export function onWidgetLevel(cb: (p: LevelPayload) => void): Promise<UnlistenFn> {
  return listen<LevelPayload>("widget-level", (e) => cb(e.payload));
}
export function onWidgetMessage(cb: (p: MessagePayload) => void): Promise<UnlistenFn> {
  return listen<MessagePayload>("widget-message", (e) => cb(e.payload));
}
