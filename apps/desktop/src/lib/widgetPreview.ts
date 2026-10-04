import type { ClientSettings, WidgetFade, WidgetIdle } from "./api";

/** A state of the widget that can be looked at from the settings window without dictating. */
export type Scene = "place" | "idle" | "listening" | "processing" | "done" | "failed";
export const SCENES: Scene[] = ["place", "idle", "listening", "processing", "done", "failed"];

/** The settings that decide what a previewed widget looks like. */
export interface Look { widget_size: ClientSettings["widget_size"]; widget_idle: WidgetIdle; widget_fade: WidgetFade; show_transcript: boolean }

export function lookOf(s: Look): Look {
  return { widget_size: s.widget_size, widget_idle: s.widget_idle, widget_fade: { ...s.widget_fade }, show_transcript: s.show_transcript };
}

/** The scene in which a changed setting can be seen; null when every scene shows it, or nothing changed. */
export function sceneFor(before: Look, after: Look): Scene | null {
  if (before.widget_idle !== after.widget_idle || JSON.stringify(before.widget_fade) !== JSON.stringify(after.widget_fade)) return "idle";
  if (before.show_transcript !== after.show_transcript) return "processing";
  return null;
}

/** One dictation played through: each scene and how long it stays, in milliseconds. The last one stays. */
export const DEMO: [Scene, number][] = [["listening", 2400], ["processing", 1800], ["done", 4200], ["idle", 0]];

/** Sample texts in the interface language. */
export interface Sample { raw: string; refined: string; detail: string }

/** What the app is asked to show. The two texts go along only when the recognised text is shown while processing. */
export function previewRequest(scene: Scene, look: Look, sample: Sample, play: boolean, faint: boolean) {
  return {
    scene,
    size: look.widget_size,
    idle: look.widget_idle,
    fade: look.widget_fade,
    raw: look.show_transcript ? sample.raw : null,
    refined: look.show_transcript ? sample.refined : null,
    detail: sample.detail,
    play,
    faint,
  };
}
export type PreviewRequest = ReturnType<typeof previewRequest>;
