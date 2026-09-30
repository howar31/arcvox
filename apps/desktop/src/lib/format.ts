import { currentLanguage } from "./i18n";

/** The locale dates and times are shown in: the interface language, not the system locale. */
export function uiLocale(): "zh-TW" | "en" {
  return currentLanguage() === "zh-TW" ? "zh-TW" : "en";
}

const at = (ts: number) => new Date(ts * 1000);

/** Hours and minutes on a 24-hour clock, from a timestamp in seconds. */
export function formatTime(ts: number): string {
  return at(ts).toLocaleTimeString(uiLocale(), { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
}

/** Month and day as numbers, from a timestamp in seconds. */
export function formatShortDate(ts: number): string {
  return at(ts).toLocaleDateString(uiLocale(), { month: "numeric", day: "numeric" });
}

/** The full date and time, from a timestamp in seconds. */
export function formatDateTime(ts: number): string {
  return at(ts).toLocaleString(uiLocale());
}

/** A calendar day as a heading: weekday, month and day, with the year when it is not the current one. */
export function formatDay(date: Date, now: Date): string {
  const opts: Intl.DateTimeFormatOptions = { month: "long", day: "numeric", weekday: "short" };
  if (date.getFullYear() !== now.getFullYear()) opts.year = "numeric";
  return date.toLocaleDateString(uiLocale(), opts);
}
