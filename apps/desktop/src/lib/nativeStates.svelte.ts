import { errorMessage, nativeStatus, type NativeStatusView, type ProviderConfig } from "./api";
import { NATIVE_DICTATION } from "./providerForm";

/** Apple dictation has a model per language; the other platform engines answer the same for every language. */
const perLanguage = (p: ProviderConfig) => p.native_id === NATIVE_DICTATION;

/**
 * The status of each platform engine (Apple dictation per language, Apple's language model) as the system reports
 * it, shared by every page that marks engines. Asked again when the settings window comes to the front: the system's
 * settings (a language installed or removed, Apple Intelligence turned off) change outside the app.
 */
class NativeStates {
  /** By engine id and language; an engine not asked yet has no entry. */
  #byKey = $state<Record<string, NativeStatusView>>({});
  #asked = new Set<string>();
  #last: { providers: ProviderConfig[]; languages: (string | null)[] } | null = null;
  #listening = false;

  #key(p: ProviderConfig, language: string | null) {
    return `${p.id}|${perLanguage(p) ? (language ?? "") : ""}`;
  }

  /** Asks for every platform engine in each of `languages` (profile language hints) not asked yet. */
  ensure(providers: ProviderConfig[], languages: (string | null)[]) {
    this.#last = { providers, languages };
    this.#listen();
    for (const p of providers) {
      if (p.kind !== "native" || !p.native_id) continue;
      for (const language of perLanguage(p) ? [...new Set(languages)] : [null]) {
        const key = this.#key(p, language);
        if (this.#asked.has(key)) continue;
        this.#asked.add(key);
        nativeStatus(p.native_id, language)
          .then((s) => { this.#byKey = { ...this.#byKey, [key]: s }; })
          .catch((e) => { this.#byKey = { ...this.#byKey, [key]: { status: "failed", reason: errorMessage(e) } }; });
      }
    }
  }

  /** Asks every platform engine again, keeping the last answers on screen until the new ones come. */
  refresh() {
    this.#asked.clear();
    if (this.#last) this.ensure(this.#last.providers, this.#last.languages);
  }

  /** The system's answer for the engine in a profile language; none before it came. */
  of(p: ProviderConfig, language: string | null): NativeStatusView | undefined {
    return this.#byKey[this.#key(p, language)];
  }

  /** Whether the engine can run in a profile language as far as the system says; not answered yet counts as able to. */
  ready(p: ProviderConfig, language: string | null): boolean {
    return p.kind !== "native" || (this.of(p, language)?.status ?? "ready") === "ready";
  }

  #listen() {
    if (this.#listening || typeof window === "undefined") return;
    this.#listening = true;
    window.addEventListener("focus", () => this.refresh());
  }
}

export const nativeStates = new NativeStates();
