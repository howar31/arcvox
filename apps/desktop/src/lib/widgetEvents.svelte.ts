import { onWidgetLevel, onWidgetMessage, onWidgetState, onWidgetTranscript, uiLanguage, widgetScale, type MessagePayload, type WidgetState } from "./api";
import { currentLanguage, setLanguage, type Lang } from "./i18n";

/** How long the hint stays when a hands-free dictation begins. */
const HINT_MS = 4000;

/** What the app last told the widget and its panel; both windows keep their own copy. */
export class WidgetFeed {
  state = $state<WidgetState>("idle");
  message = $state<MessagePayload | null>(null);
  /** Recognised text, while the rewrite runs. */
  transcript = $state("");
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
        if (p.state !== "idle" && p.state !== "placing") this.message = null;
        if (p.state !== "processing" && p.state !== "inserting") this.transcript = "";
        if (p.state !== "recording" && p.state !== "locked") this.level = 0;
      }),
      onWidgetLevel((p) => { this.level = p.rms; }),
      onWidgetMessage((p) => { this.setLanguage(p.language); this.message = p; this.transcript = ""; }),
      onWidgetTranscript((p) => { this.transcript = p.text; }),
    ];
    return () => {
      clearTimeout(hintTimer);
      unsubs.forEach((u) => u.then((f) => f()));
    };
  }
}
