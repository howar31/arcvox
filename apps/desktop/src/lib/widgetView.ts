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
  moving: "widget.placing",
};
/** Messages the widget shows by itself, with their label. */
const NOTICES: Record<string, string> = {
  "widget.cancelled": "widget.cancelled",
  "widget.no_speech": "widget.no_speech_short",
};
/** The label of a widget that stays on screen while no dictation goes on. */
export const READY = "widget.ready";
const INSERTED = "widget.inserted";
const FAILED = "widget.failed";
const NOT_INSERTED = "widget.not_inserted";
const REWRITE_FAILED = "widget.rewrite_failed";

/** Every label the widget can show; its long side holds the longest of them. */
export const LABEL_KEYS: string[] = [...Object.values(STATE_LABELS), ...Object.values(NOTICES), READY, INSERTED, FAILED, NOT_INSERTED, REWRITE_FAILED];

/** The text exists but is not in the application, or is not what the processing should have made of it. */
function needsAttention(message: MessagePayload): boolean {
  return message.key === "widget.degraded" || (message.kind === "info" && !!message.copyable);
}

/**
 * The key of the widget's label, or null when it shows the ring alone.
 * `inserted`: the text was put into the application and nothing happened since.
 */
export function labelKey(state: WidgetState, message: MessagePayload | null, inserted = false): string | null {
  if (!message) return STATE_LABELS[state] ?? (inserted && state === "idle" ? INSERTED : null);
  if (message.key in NOTICES) return NOTICES[message.key];
  if (message.key === "widget.degraded") return REWRITE_FAILED;
  return needsAttention(message) ? NOT_INSERTED : FAILED;
}

/**
 * The key of the line the settings window shows while a dictation goes on, or null when none does.
 * The same labels as the widget's, except that hands-free dictation has room for a full phrase.
 */
export function dictationLabelKey(state: WidgetState): string | null {
  if (state === "placing" || state === "moving") return null;
  if (state === "locked") return "shell.hands_free";
  return STATE_LABELS[state] ?? null;
}

export interface RingView {
  /** The colour: the kind of outcome. Listening and working share one, red is for a failure only. */
  tone: "accent" | "ok" | "quiet" | "warn" | "fail";
  /** What the ring does when this begins; `level`, `spin` and `drag` go on for as long as it lasts. */
  motion: "level" | "spin" | "close" | "once" | "twice" | "settle" | "shake" | "drag" | "none";
  glyph: "check" | "bang" | "cross" | null;
}

/** The ring of a widget at rest. */
export const RING_AT_REST: RingView = { tone: "quiet", motion: "none", glyph: null };

export function ringView(state: WidgetState, message: MessagePayload | null, inserted: boolean): RingView {
  if (message) {
    if (message.key in NOTICES) return { tone: "quiet", motion: "settle", glyph: null };
    if (needsAttention(message)) return { tone: "warn", motion: "twice", glyph: "bang" };
    return { tone: "fail", motion: "shake", glyph: "cross" };
  }
  if (state === "recording" || state === "locked") return { tone: "accent", motion: "level", glyph: null };
  if (state === "processing") return { tone: "accent", motion: "spin", glyph: null };
  if (state === "inserting") return { tone: "accent", motion: "close", glyph: null };
  if (state === "placing" || state === "moving") return { tone: "quiet", motion: "drag", glyph: null };
  if (inserted) return { tone: "ok", motion: "once", glyph: "check" };
  return { tone: "quiet", motion: "none", glyph: null };
}

export type PanelAction = "copy" | "copy_detail" | "open_engines" | "open_history";

export interface PanelView {
  /** A small line above the text. */
  caption?: string;
  title?: string;
  /** Running text without a title: the recognised text, a hint. */
  text?: string;
  /** The recognised text and the final text it turned into. */
  rewrite?: { from: string; to: string };
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
/** How the widget settles where it is dragged, one sentence a line. */
const PLACING_HINTS = ["panel.placing_center", "panel.placing_side", "panel.placing_bottom", "panel.placing_panel"];
/** Messages that are settled on the engines page. */
const TO_ENGINES = new Set(["widget.engine_unavailable", "widget.model_missing", "widget.model_corrupt"]);

/**
 * What the panel shows, or null when the widget needs no panel. `refined` is the final text once the
 * processing is over; `hint` is true while a hands-free dictation has just begun.
 */
export function panelView(state: WidgetState, message: MessagePayload | null, transcript: string, refined: string | null, hint: boolean): PanelView | null {
  if (!message) {
    if (transcript && refined !== null) return { caption: t("panel.final"), rewrite: { from: transcript, to: refined }, actions: [], close: false };
    if (state === "processing" && transcript) return { caption: t("panel.recognised"), text: transcript, actions: [], close: false };
    if (state === "locked" && hint) return { text: t("widget.locked"), actions: [], close: false };
    // Shown from the settings window to be positioned: the panel is where it will be, and says how the widget settles.
    if (state === "placing") return { title: t("panel.placing"), body: PLACING_HINTS.map((key) => t(key)).join("\n"), actions: [], close: false };
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
