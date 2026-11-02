import { currentLanguage, t } from "./i18n";

/** A language tag (BCP 47) in the interface language, for example 「中文（台灣）」 for `zh-TW`. */
export function languageName(tag: string | null): string {
  if (!tag) return t("profiles.lang.zh_tw");
  try {
    const name = new Intl.DisplayNames([currentLanguage()], { type: "language" }).of(tag);
    return name && name !== tag ? name : tag;
  } catch {
    return tag;
  }
}

/**
 * The dictation languages to list: the ones kept for the app and the ones profiles use, each once, in name order.
 * A language the system has from elsewhere and no profile uses is left out, so a removed one leaves the list at once.
 */
export function shownLanguages(langs: { reserved: string[] }, used: string[]): string[] {
  const all = new Set([...langs.reserved, ...used]);
  return [...all].sort((a, b) => languageName(a).localeCompare(languageName(b)));
}
