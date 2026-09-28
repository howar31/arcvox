import zhTW from "./i18n/zh-TW.json";
import en from "./i18n/en.json";

export type Lang = "zh-TW" | "en";
const tables: Record<Lang, Record<string, string>> = { "zh-TW": zhTW, en };
let current: Lang = "en";

export function setLanguage(lang: Lang): void {
  current = lang;
  if (typeof document !== "undefined") {
    document.documentElement.lang = lang === "zh-TW" ? "zh-Hant-TW" : "en";
  }
}

export function currentLanguage(): Lang {
  return current;
}

export function t(key: string, vars: Record<string, string> = {}): string {
  const raw = tables[current][key] ?? tables.en[key] ?? key;
  return raw.replace(/\{(\w+)\}/g, (_, name: string) => vars[name] ?? `{${name}}`);
}

export function keysOf(lang: Lang): string[] {
  return Object.keys(tables[lang]);
}
