import { onWidgetCopied, onWidgetEnter, onWidgetInserted, onWidgetLeave, onWidgetLevel, onWidgetMessage, onWidgetRefined, onWidgetState, onWidgetTranscript, uiLanguage, widgetIdle, widgetScale, type MessagePayload, type WidgetIdle, type WidgetState } from "./api";
import { currentLanguage, setLanguage, type Lang } from "./i18n";
import { messageDelay } from "./rewriteTiming";
import type { Done } from "./widgetView";

/** How long the hint stays when a hands-free dictation begins. */
const HINT_MS = 4000;

/** What the app last told the widget and its panel; both windows keep their own copy. */
export class WidgetFeed {
  state = $state<WidgetState>("idle");
  message = $state<MessagePayload | null>(null);
  /** True from the moment the widget is shown until it is told to leave; its window stays for the leaving motion. */
  open = $state(false);
  /** Recognised text, from the moment it is known until the widget goes. */
  transcript = $state("");
  /** The final text, once the processing is over. */
  refined = $state<string | null>(null);
  /** True from the moment the text was put into the application until something else happens. */
  done = $state<Done>(null);
  /** When the final text arrived. */
  private refinedAt = 0;
  /** The loudest input level since it was last taken, and whether one arrived at all. */
  private peak = 0;
  private fresh = false;
  private lastLevel = 0;
  /** True for a moment after a hands-free dictation began. */
  hint = $state(false);
  /** Changes with the interface language, so texts are read again. */
  language = $state<Lang>(currentLanguage());

  /** The factor of the size the user chose; every measure of the widget and the panel is a multiple of the unit it sets. */
  scale = $state(1);
  /** What the widget shows while it is not open. */
  idle = $state<WidgetIdle>("hidden");

  private setScale(scale: number | undefined) {
    if (!scale || scale === this.scale) return;
    document.body.style.setProperty("--u", scale + "px");
    this.scale = scale;
  }

  private setLanguage(lang: Lang) {
    setLanguage(lang);
    this.language = lang;
  }

  /** The input level (RMS) since the last call; the previous one when no block of audio arrived in between. */
  takeLevel(): number {
    if (this.fresh) this.lastLevel = this.peak;
    this.peak = 0;
    this.fresh = false;
    return this.lastLevel;
  }

  /** Starts listening; returns the function that stops it. */
  listen(): () => void {
    // Until the language is known the texts are in English; each event carries it too.
    uiLanguage().then((l) => this.setLanguage(l as Lang)).catch(() => {});
    widgetScale().then((s) => this.setScale(s)).catch(() => {});
    widgetIdle().then((i) => { this.idle = i; }).catch(() => {});
    let hintTimer: ReturnType<typeof setTimeout> | undefined;
    // A message that waits for the change of the text to be seen.
    let waiting: ReturnType<typeof setTimeout> | undefined;
    const still = window.matchMedia("(prefers-reduced-motion: reduce)");
    const show = (p: MessagePayload) => { this.message = p; this.transcript = ""; this.refined = null; this.done = null; };
    const unsubs = [
      onWidgetState((p) => {
        this.setLanguage(p.language);
        this.setScale(p.scale);
        if (p.idle) this.idle = p.idle;
        if (p.state === "locked" && this.state !== "locked") {
          this.hint = true;
          clearTimeout(hintTimer);
          hintTimer = setTimeout(() => { this.hint = false; }, HINT_MS);
        } else if (p.state !== "locked") this.hint = false;
        this.state = p.state;
        if (p.state !== "idle") this.done = null;
        // A new dictation drops a message that was still waiting.
        if (p.state === "recording" || p.state === "locked" || p.state === "processing") clearTimeout(waiting);
        if (p.state !== "idle" && p.state !== "placing" && p.state !== "moving") this.message = null;
        // The two texts stay through the insertion and the moment after it; a new dictation clears them.
        if (p.state === "recording" || p.state === "locked" || p.state === "placing" || p.state === "moving") this.refined = null;
        if (p.state !== "processing" && p.state !== "inserting" && this.refined === null) this.transcript = "";
        if (p.state !== "recording" && p.state !== "locked") this.peak = this.lastLevel = 0;
      }),
      onWidgetLevel((p) => { this.peak = Math.max(this.peak, p.rms); this.fresh = true; }),
      onWidgetMessage((p) => {
        this.setLanguage(p.language);
        clearTimeout(waiting);
        // The final text is shown taking the place of the recognised text before a message takes the panel.
        const wait = messageDelay(this.transcript, this.refined, this.refinedAt, performance.now(), still.matches);
        if (wait > 0) waiting = setTimeout(() => show(p), wait);
        else show(p);
      }),
      onWidgetInserted(() => { this.done = "inserted"; }),
      onWidgetCopied(() => { this.done = "copied"; }),
      onWidgetEnter(() => { this.open = true; }),
      onWidgetLeave(() => { this.open = false; }),
      onWidgetTranscript((p) => { this.transcript = p.text; this.refined = null; }),
      onWidgetRefined((p) => {
        if (!this.transcript) return;
        this.refined = p.text;
        this.refinedAt = performance.now();
      }),
    ];
    return () => {
      clearTimeout(hintTimer);
      clearTimeout(waiting);
      unsubs.forEach((u) => u.then((f) => f()));
    };
  }
}
