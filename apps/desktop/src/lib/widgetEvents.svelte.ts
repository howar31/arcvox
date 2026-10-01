import { onWidgetLevel, onWidgetMessage, onWidgetRefined, onWidgetState, onWidgetTranscript, uiLanguage, widgetScale, type MessagePayload, type WidgetState } from "./api";
import { currentLanguage, setLanguage, type Lang } from "./i18n";
import { messageDelay } from "./rewriteTiming";

/** How long the hint stays when a hands-free dictation begins. */
const HINT_MS = 4000;

/** What the app last told the widget and its panel; both windows keep their own copy. */
export class WidgetFeed {
  state = $state<WidgetState>("idle");
  message = $state<MessagePayload | null>(null);
  /** Recognised text, from the moment it is known until the widget goes. */
  transcript = $state("");
  /** The final text, once the processing is over. */
  refined = $state<string | null>(null);
  /** When the final text arrived. */
  private refinedAt = 0;
  level = $state(0);
  /** True for a moment after a hands-free dictation began. */
  hint = $state(false);
  /** Changes with the interface language, so texts are read again. */
  language = $state<Lang>(currentLanguage());

  /** The factor of the size the user chose; every measure of the widget and the panel is a multiple of the unit it sets. */
  scale = $state(1);

  private setScale(scale: number | undefined) {
    if (!scale || scale === this.scale) return;
    document.body.style.setProperty("--u", scale + "px");
    this.scale = scale;
  }

  private setLanguage(lang: Lang) {
    setLanguage(lang);
    this.language = lang;
  }

  /** Starts listening; returns the function that stops it. */
  listen(): () => void {
    // Until the language is known the texts are in English; each event carries it too.
    uiLanguage().then((l) => this.setLanguage(l as Lang)).catch(() => {});
    widgetScale().then((s) => this.setScale(s)).catch(() => {});
    let hintTimer: ReturnType<typeof setTimeout> | undefined;
    // A message that waits for the change of the text to be seen.
    let waiting: ReturnType<typeof setTimeout> | undefined;
    const still = window.matchMedia("(prefers-reduced-motion: reduce)");
    const show = (p: MessagePayload) => { this.message = p; this.transcript = ""; this.refined = null; };
    const unsubs = [
      onWidgetState((p) => {
        this.setLanguage(p.language);
        this.setScale(p.scale);
        if (p.state === "locked" && this.state !== "locked") {
          this.hint = true;
          clearTimeout(hintTimer);
          hintTimer = setTimeout(() => { this.hint = false; }, HINT_MS);
        } else if (p.state !== "locked") this.hint = false;
        this.state = p.state;
        // A new dictation drops a message that was still waiting.
        if (p.state === "recording" || p.state === "locked" || p.state === "processing") clearTimeout(waiting);
        if (p.state !== "idle" && p.state !== "placing") this.message = null;
        // The two texts stay through the insertion and the moment after it; a new dictation clears them.
        if (p.state === "recording" || p.state === "locked" || p.state === "placing") this.refined = null;
        if (p.state !== "processing" && p.state !== "inserting" && this.refined === null) this.transcript = "";
        if (p.state !== "recording" && p.state !== "locked") this.level = 0;
      }),
      onWidgetLevel((p) => { this.level = p.rms; }),
      onWidgetMessage((p) => {
        this.setLanguage(p.language);
        clearTimeout(waiting);
        // The final text is shown taking the place of the recognised text before a message takes the panel.
        const wait = messageDelay(this.transcript, this.refined, this.refinedAt, performance.now(), still.matches);
        if (wait > 0) waiting = setTimeout(() => show(p), wait);
        else show(p);
      }),
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
