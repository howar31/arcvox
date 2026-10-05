export type Page = "home" | "history" | "flow" | "dictionary" | "prompt" | "engines" | "speed" | "general" | "recording" | "data" | "backup" | "advanced" | "about";

/** The pages of settings: one set of settings shown in parts. */
export const SETTINGS_PAGES = ["general", "recording", "data", "advanced"] as const;
export type SettingsPage = (typeof SETTINGS_PAGES)[number];

/** Sidebar layout: an optional group label key and its pages, in order. */
export const NAV: { group: string | null; pages: Page[] }[] = [
  { group: null, pages: ["home", "history"] },
  { group: "nav.group.dictation", pages: ["flow", "engines", "prompt", "dictionary"] },
  { group: "nav.group.settings", pages: ["general", "recording", "data", "backup", "advanced"] },
  { group: null, pages: ["about"] },
];

/** Pages without a sidebar entry, and the entry that stands for each. */
const UNDER: Partial<Record<Page, Page>> = { speed: "engines" };

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

/**
 * Whether a hash change that did not go through the guard (for example one from the tray)
 * has to wait for the user: only when the page has unsaved changes and the route really changes.
 */
export function shouldIntercept(dirty: boolean, current: Route, next: Route): boolean {
  return dirty && (current.page !== next.page || current.arg !== next.arg);
}
