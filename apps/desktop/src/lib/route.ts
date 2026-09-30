export type Page = "home" | "profiles" | "engines" | "dictionary" | "prompt" | "history" | "general" | "speed" | "about";

/** Sidebar layout: an optional group label key and its pages, in order. */
export const NAV: { group: string | null; pages: Page[] }[] = [
  { group: null, pages: ["home"] },
  { group: "nav.group.dictation", pages: ["profiles", "engines", "dictionary", "prompt", "history"] },
  { group: "nav.group.app", pages: ["general", "speed", "about"] },
];

const PAGES = new Set<string>(NAV.flatMap((g) => g.pages));
/** Earlier page names that now live on another page. */
const ALIASES: Record<string, Page> = { providers: "engines", models: "engines" };

/** Reads `#<page>` or `#<page>/<arg>`; unknown pages resolve to the overview. */
export function parseRoute(hash: string): { page: Page; arg: string | null } {
  const body = hash.replace(/^#/, "");
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

export function routeHash(page: Page, arg?: string | null): string {
  return arg ? `#${page}/${encodeURIComponent(arg)}` : `#${page}`;
}

export function go(page: Page, arg?: string | null): void {
  location.hash = routeHash(page, arg);
}

export type Route = ReturnType<typeof parseRoute>;

/**
 * Whether a hash change that did not go through the guard (for example one from the tray)
 * has to wait for the user: only when the page has unsaved changes and the route really changes.
 */
export function shouldIntercept(dirty: boolean, current: Route, next: Route): boolean {
  return dirty && (current.page !== next.page || current.arg !== next.arg);
}
