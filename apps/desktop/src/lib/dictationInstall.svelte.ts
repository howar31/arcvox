import { errorMessage, nativeInstallLanguage, onNativeInstallProgress } from "./api";
import { t } from "./i18n";
import { languageName } from "./languages";
import { nativeStates } from "./nativeStates.svelte";
import { toast } from "./ui/toast.svelte";

const STATUSES = ["needs_download", "locale_unsupported", "os_too_old", "failed"];

/**
 * The Apple dictation language being downloaded, with the system's progress; kept outside the page, so leaving the
 * page or the tab and coming back shows the download still going. One at a time.
 */
class DictationInstall {
  locale = $state<string | null>(null);
  fraction = $state(0);
  /** Counts finished downloads, for lists to read the languages again. */
  done = $state(0);
  #listening = false;

  async run(locale: string) {
    if (this.locale) return;
    this.#listen();
    this.locale = locale;
    this.fraction = 0;
    try {
      await nativeInstallLanguage(locale);
      toast.ok(t("engines.language_downloaded", { language: languageName(locale) }));
    } catch (e) {
      const m = errorMessage(e);
      toast.error(STATUSES.includes(m) ? t(`engines.native.${m}`) : m);
    }
    this.locale = null;
    this.done += 1;
    nativeStates.refresh();
  }

  #listen() {
    if (this.#listening) return;
    this.#listening = true;
    onNativeInstallProgress((f) => { if (this.locale) this.fraction = f; });
  }
}

export const dictationInstall = new DictationInstall();
