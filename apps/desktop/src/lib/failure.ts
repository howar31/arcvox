import { has, t } from "./i18n";

/** A failure in the interface language when its kind has a text, else the detail the engine gave. */
export function failureText(code: string | null, detail: string | null): string {
  return code && has("failure." + code) ? t("failure." + code) : (detail ?? "");
}
