import { has, t } from "./i18n";

/** A failure in the interface language when its kind has a text, else the detail the engine gave. */
export function failureText(code: string | null, detail: string | null): string {
  return code && has("failure." + code) ? t("failure." + code) : (detail ?? "");
}

/** The short form for the widget, or null when the kind has none (its headline already says it). */
export function failureShort(code: string | null): string | null {
  return code && has("failure_short." + code) ? t("failure_short." + code) : null;
}
