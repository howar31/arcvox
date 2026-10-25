import type { Release } from "./api";
import { releaseNotes, type Note } from "./releaseNotes";

/** One release on the What's new page. */
export interface Section { version: string; notes: Note[] }

/** The detail goes on a line of its own under the lead, so it starts as a sentence. */
function stacked(n: Note): Note {
  return n.lead && n.detail ? { lead: n.lead, detail: n.detail[0].toUpperCase() + n.detail.slice(1) } : n;
}

/** The releases with notes in `lang`, in the order given (newest first); a release without notes drops out. */
export function sections(releases: Release[], lang: string): Section[] {
  return releases.map((r) => ({ version: r.version, notes: releaseNotes(r.notes, lang).map(stacked) })).filter((s) => s.notes.length > 0);
}

/** The page's title: the one version, or the oldest and the newest of several. */
export function title(list: Section[], t: (key: string, params: Record<string, string>) => string): string {
  const v = titleVersions(list);
  if (!v) return "";
  return "version" in v ? t("about.update.notes", v) : t("about.update.notes_range", v);
}

/** The versions a title names: one release, or the oldest and the newest of several. */
export function titleVersions(list: Section[]): { version: string } | { from: string; to: string } | null {
  if (list.length === 0) return null;
  if (list.length === 1) return { version: list[0].version };
  return { from: list[list.length - 1].version, to: list[0].version };
}
