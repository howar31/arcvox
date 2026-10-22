import { SvelteMap } from "svelte/reactivity";
import { appIcons, appNames } from "./api";

// Resolved application names by bundle id; null when the app is not installed.
const names = new SvelteMap<string, string | null>();
const pending = new Set<string>();
// Application icons as data URLs by bundle id; null when the app has none.
const icons = new SvelteMap<string, string | null>();
const pendingIcons = new Set<string>();

/** The application's name, or the bundle id until (or unless) it resolves. */
export function appName(bundleId: string): string {
  return names.get(bundleId) ?? bundleId;
}

/** The application's icon, or null until (or unless) it resolves. */
export function appIcon(bundleId: string): string | null {
  return icons.get(bundleId) ?? null;
}

/** Looks up the names not asked for yet; a failed lookup is tried again next time. */
export async function ensureAppNames(ids: string[]): Promise<void> {
  await ensure(ids, names, pending, appNames);
}

// Icons asked for in the same tick go out in one request.
const queuedIcons = new Set<string>();
let flush: Promise<void> | null = null;

/** Looks up the icons not asked for yet, in one request per tick; a failed lookup is tried again next time. */
export function ensureAppIcons(ids: string[]): Promise<void> {
  ids.forEach((id) => queuedIcons.add(id));
  flush ??= Promise.resolve().then(() => {
    const batch = [...queuedIcons];
    queuedIcons.clear();
    flush = null;
    return ensure(batch, icons, pendingIcons, appIcons);
  });
  return flush;
}

async function ensure(
  ids: string[],
  known: SvelteMap<string, string | null>,
  inFlight: Set<string>,
  lookup: (ids: string[]) => Promise<Record<string, string | null>>,
): Promise<void> {
  const missing = [...new Set(ids)].filter((id) => !known.has(id) && !inFlight.has(id));
  if (missing.length === 0) return;
  missing.forEach((id) => inFlight.add(id));
  try {
    const found = await lookup(missing);
    for (const id of missing) known.set(id, found[id] ?? null);
  } catch {
    // Keep showing what is known.
  } finally {
    missing.forEach((id) => inFlight.delete(id));
  }
}
