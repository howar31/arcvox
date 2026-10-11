import { onDictationPaused, pauseSet, pauseState } from "./api";

/** Whether dictation is paused, as the menu-bar menu's item shows; shared by the pages that say so. */
let value = $state(false);
let watched: Promise<() => void> | null = null;

export const pause = {
  get value() { return value; },
  /** Reads the state once and follows its changes; later callers share the listener. */
  watch() {
    pauseState().then((p) => { value = p; }).catch(() => {});
    if (!watched) watched = onDictationPaused((p) => { value = p; });
  },
  resume() { return pauseSet(false); },
};
