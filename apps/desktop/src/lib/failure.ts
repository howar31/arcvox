import type { Stage } from "./api";
import { has, t } from "./i18n";

/** A failure in the interface language when its kind has a text, else the detail the engine gave. */
export function failureText(code: string | null, detail: string | null): string {
  return code && has("failure." + code) ? t("failure." + code) : (detail ?? "");
}

/** What the service said about a failure, from a stage's record: its status and words, and its answer as it came. */
export function serviceWords(detail: Record<string, unknown>): { body: string | null; detail: string | null } {
  const status = typeof detail.status === "number" ? `HTTP ${detail.status}` : null;
  const answer = typeof detail.answer === "string" ? detail.answer : null;
  return {
    body: status && answer ? `${status}: ${answer}` : (status ?? answer),
    detail: typeof detail.raw === "string" ? detail.raw : null,
  };
}

/** What the service said about a failed row, from the stage it failed at. */
export const stageFailure = (stages: Stage[] | null | undefined) => serviceWords(stages?.find((s) => s.text === null && s.detail.error !== undefined)?.detail ?? {});
