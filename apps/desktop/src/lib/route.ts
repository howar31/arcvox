export type Page = "home" | "history" | "flow" | "dictionary" | "prompt" | "engines" | "speed" | "quality" | "general" | "recording" | "data" | "storage" | "backup" | "about" | "licenses" | "whatsnew" | "report" | "welcome";

/** The pages of settings: one set of settings shown in parts. */
export const SETTINGS_PAGES = ["general", "recording", "data", "storage"] as const;
export type SettingsPage = (typeof SETTINGS_PAGES)[number];

/** The pages with a sidebar entry (and an icon of their own). */
export type NavPage = Exclude<Page, "speed" | "quality" | "licenses" | "whatsnew" | "report" | "prompt" | "welcome">;

/** Sidebar layout: an optional group label key and its pages, in order. */
export const NAV: { group: string | null; pages: NavPage[] }[] = [
  { group: null, pages: ["home", "history"] },
  { group: "nav.group.dictation", pages: ["flow", "engines", "dictionary"] },
  { group: "nav.group.settings", pages: ["general", "recording", "data", "storage", "backup"] },
  { group: null, pages: ["about"] },
];

/** Pages without a sidebar entry, and the entry that stands for each. */
const UNDER: Partial<Record<Page, Page>> = { speed: "engines", quality: "engines", licenses: "about", whatsnew: "about", report: "about", prompt: "flow", welcome: "home" };

/** The sidebar entry that is current while `page` is shown. */
export function navPage(page: Page): Page {
  return UNDER[page] ?? page;
}

const PAGES = new Set<string>([...NAV.flatMap((g) => g.pages), ...Object.keys(UNDER)]);
/** Earlier page names that now live on another page. */
const ALIASES: Record<string, Page> = { providers: "engines", models: "engines", profiles: "flow" };

/** A hash may end in `@<section>`: the part of the page to bring into view. */
function split(hash: string): [string, string | null] {
  const body = hash.replace(/^#/, "");
  const at = body.lastIndexOf("@");
  return at < 0 ? [body, null] : [body.slice(0, at), body.slice(at + 1) || null];
}

/** The section a hash names, on whatever page it is. */
export function routeSection(hash: string): string | null {
  return split(hash)[1];
}

/** Reads `#<page>` or `#<page>/<arg>`, with or without a section; unknown pages resolve to the home page. */
export function parseRoute(hash: string): { page: Page; arg: string | null } {
  const [body] = split(hash);
  const slash = body.indexOf("/");
  const name = slash < 0 ? body : body.slice(0, slash);
  const rawArg = slash < 0 ? "" : body.slice(slash + 1);
  const page = ALIASES[name] ?? (PAGES.has(name) ? (name as Page) : null);
  if (!page) return { page: "home", arg: null };
  let arg: string | null = null;
  if (rawArg) {
    try { arg = decodeURIComponent(rawArg); } catch { arg = rawArg; }
  }
  return { page, arg };
}

export function routeHash(page: Page, arg?: string | null, section?: string | null): string {
  const at = section ? `@${section}` : "";
  return (arg ? `#${page}/${encodeURIComponent(arg)}` : `#${page}`) + at;
}

export function go(page: Page, arg?: string | null, section?: string | null): void {
  const hash = routeHash(page, arg, section);
  // The same hash again announces nothing by itself, and a section still has to be scrolled to.
  if (location.hash === hash) window.dispatchEvent(new Event("hashchange"));
  else location.hash = hash;
}

export type Route = ReturnType<typeof parseRoute>;

/** How far a route sits under its sidebar entry: 0 on the entry's own page, 1 on a page under it, 2 under that. */
export function depth(r: Route): number {
  switch (r.page) {
    case "whatsnew": case "report": case "prompt": case "speed": return 1;
    case "licenses": return r.arg ? 2 : 1;
    case "quality": return r.arg === "result" || r.arg === "compare" ? 2 : 1;
    default: return 0;
  }
}

/** 1 into a page under the current one, -1 back up towards the entry, 0 to another entry or at the same depth. */
export function direction(from: Route, to: Route): -1 | 0 | 1 {
  if (navPage(from.page) !== navPage(to.page)) return 0;
  return Math.sign(depth(to) - depth(from)) as -1 | 0 | 1;
}

/**
 * Whether a hash change that did not go through the guard (for example one from the tray)
 * has to wait for the user: only when the page has unsaved changes and the route really changes.
 */
export function shouldIntercept(dirty: boolean, current: Route, next: Route): boolean {
  return dirty && (current.page !== next.page || current.arg !== next.arg);
}
