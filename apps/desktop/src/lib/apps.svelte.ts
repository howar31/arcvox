import { SvelteMap } from "svelte/reactivity";
import { appNames } from "./api";

// Resolved application names by bundle id; null when the app is not installed.
const names = new SvelteMap<string, string | null>();
const pending = new Set<string>();

/** The application's name, or the bundle id until (or unless) it resolves. */
export function appName(bundleId: string): string {
  return names.get(bundleId) ?? bundleId;
}

/** Looks up the names not asked for yet; a failed lookup is tried again next time. */
export async function ensureAppNames(ids: string[]): Promise<void> {
  const missing = [...new Set(ids)].filter((id) => !names.has(id) && !pending.has(id));
  if (missing.length === 0) return;
  missing.forEach((id) => pending.add(id));
  try {
    const found = await appNames(missing);
    for (const id of missing) names.set(id, found[id] ?? null);
  } catch {
    // Keep showing the bundle ids.
  } finally {
    missing.forEach((id) => pending.delete(id));
  }
}
