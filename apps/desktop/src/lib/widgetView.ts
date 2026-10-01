import type { MessagePayload, WidgetState } from "./api";
import { failureText } from "./failure";
import { t } from "./i18n";

// The widget shows a ring and one short label. Everything with more to read,
// or with a button, is in the panel next to it.

const STATE_LABELS: Partial<Record<WidgetState, string>> = {
  recording: "widget.listening",
  locked: "widget.locked_short",
  processing: "widget.processing",
  inserting: "widget.inserting",
  placing: "widget.placing",
};
/** Messages the widget shows by itself, with their label. */
const NOTICES: Record<string, string> = {
  "widget.cancelled": "widget.cancelled",
  "widget.no_speech": "widget.no_speech_short",
};
const FAILED = "widget.failed";
const NOT_INSERTED = "widget.not_inserted";
const REWRITE_FAILED = "widget.rewrite_failed";

/** Every label the widget can show; its long side holds the longest of them. */
export const LABEL_KEYS: string[] = [...Object.values(STATE_LABELS), ...Object.values(NOTICES), FAILED, NOT_INSERTED, REWRITE_FAILED];

/** The key of the widget's label, or null when it shows the ring alone. */
export function labelKey(state: WidgetState, message: MessagePayload | null): string | null {
  if (!message) return STATE_LABELS[state] ?? null;
  if (message.key in NOTICES) return NOTICES[message.key];
  if (message.key === "widget.degraded") return REWRITE_FAILED;
  return message.kind === "info" && message.copyable ? NOT_INSERTED : FAILED;
}

export type PanelAction = "copy" | "copy_detail" | "open_engines" | "open_history";

export interface PanelView {
  /** A small line above the text. */
  caption?: string;
  title?: string;
  /** Running text without a title: the recognised text, a hint. */
  text?: string;
  body?: string;
  /** The dictated text that was not inserted. */
  result?: string;
  /** The engine's or the system's own words. */
  detail?: string;
  actions: PanelAction[];
  close: boolean;
}

/** What a message says under its title: a text of its own, or nothing when the title says it all. */
const BODIES: Record<string, string | null> = {
  "widget.microphone_unavailable": "panel.microphone",
  "widget.model_missing": "panel.model_missing",
  "widget.model_corrupt": "panel.model_corrupt",
  "widget.native_unavailable": null,
  "widget.empty_result": null,
};
/** Messages that are settled on the engines page. */
const TO_ENGINES = new Set(["widget.engine_unavailable", "widget.model_missing", "widget.model_corrupt"]);

/** What the panel shows, or null when the widget needs no panel. `hint` is true while a hands-free dictation has just begun. */
export function panelView(state: WidgetState, message: MessagePayload | null, transcript: string, hint: boolean): PanelView | null {
  if (!message) {
    if (state === "processing" && transcript) return { caption: t("panel.recognised"), text: transcript, actions: [], close: false };
    if (state === "locked" && hint) return { text: t("widget.locked"), actions: [], close: false };
    return null;
  }
  if (message.key in NOTICES) return null;
  const title = t(message.key);
  if (message.kind === "info" && message.copyable) return { title, result: message.copyable, actions: ["copy"], close: true };
  if (message.key === "widget.degraded") return { title, detail: message.detail ?? undefined, actions: [], close: false };
  const own = BODIES[message.key];
  const reason = message.key in BODIES ? (own ? t(own) : "") : failureText(message.code, null);
  // One sentence a line.
  const body = [reason, message.retry ? t("panel.kept") : ""].filter(Boolean).join("\n");
  const actions: PanelAction[] = [];
  if (message.copyable) actions.push("copy_detail");
  if (TO_ENGINES.has(message.key)) actions.push("open_engines");
  if (message.retry) actions.push("open_history");
  return { title, body: body || undefined, detail: message.copyable ?? message.detail ?? undefined, actions, close: !!message.copyable };
}
