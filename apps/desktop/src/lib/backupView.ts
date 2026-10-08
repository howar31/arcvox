import { formatBytes, type BackupCategories, type BackupDevice, type BackupStatus, type FolderState, type ImportReport, type SettingsNote, type WriteReport } from "./api";
import { has, t } from "./i18n";
import type { Page } from "./route";

/** The two kinds of data a settings file can hold, and the items of each in the order the dialogs list them. */
export const KIND_ITEMS = {
  settings: ["recording", "insertion", "general", "appearance"],
  content: ["profiles", "providers", "dictionary", "base_prompt"],
} as const;
export type FileKind = keyof typeof KIND_ITEMS;
/** What is backed up to the folder, in the order of the three kinds: settings, content, records. */
export const CATEGORIES = ["settings", "content", "history", "stats", "recordings"] as const;

const n = (v: number) => v.toLocaleString();

/** What is backed up to the folder, as one line. */
export function itemsSummary(c: BackupCategories): string {
  const names = CATEGORIES.filter((k) => c[k]).map((k) => t(`backup.cat.${k}`));
  return names.length ? names.join(t("backup.list_sep")) : t("backup.items_none");
}

export interface NoteLine { text: string; detail?: string; go?: Page }

/** The notes of a settings import as lines; a line may lead to a page. */
export function noteLines(notes: SettingsNote[]): NoteLine[] {
  return notes.map((note): NoteLine => {
    switch (note.kind) {
      case "needs_key": return { text: t("backup.note.needs_key", { engine: note.engine }), detail: t("backup.note.needs_key_detail"), go: "engines" };
      case "engine_replaced": return { text: t(note.role === "asr" ? "backup.note.engine_replaced_asr" : "backup.note.engine_replaced_llm", { profile: note.profile }) };
      case "engine_unavailable": return { text: t("backup.note.engine_unavailable", { engine: note.engine }) };
      case "unknown_section": return { text: t("backup.note.unknown_section", { name: note.name }) };
      case "apps_not_installed": return { text: t("backup.note.apps", { n: n(note.names.length), names: note.names.join(t("backup.list_sep")) }), detail: t("backup.note.apps_detail") };
    }
  });
}

export function platformName(platform: string): string {
  return has(`backup.platform.${platform}`) ? t(`backup.platform.${platform}`) : platform;
}

/** A device of a backup: its platform, rows, months and recordings. */
export function deviceLine(d: BackupDevice): string {
  const parts = [t("backup.ib_rows", { n: n(d.rows) })];
  if (d.first_month && d.last_month) parts.push(d.first_month === d.last_month ? d.first_month : t("backup.ib_range", { from: d.first_month, to: d.last_month }));
  parts.push(d.recordings ? t("backup.ib_recordings", { n: n(d.recordings), size: formatBytes(d.recording_bytes) }) : t("backup.ib_no_recordings"));
  return t("backup.ib_line", { platform: platformName(d.platform), parts: parts.join(t("backup.part_sep")) });
}

/** What the folder row offers for a folder that cannot be used: making it again, or the system's settings. */
export type FolderFix = "create" | "system";
const FOLDER_FIX: Record<Exclude<FolderState, "usable">, FolderFix | null> = { missing: "create", not_writable: null, blocked: "system" };
/** How often the page looks whether the folder is still there, while it is open. */
export const FOLDER_LOOK_MS = 5000;

/** `fix` belongs to the folder row; `run` says whether a write can be asked for (not while the folder is gone). */
export interface StatusView { tone: "idle" | "ok" | "busy" | "bad"; title: string; detail: string; fix: FolderFix | null; run: boolean }

/** The state of the backup folder; `time` formats a timestamp in seconds. */
/** How long "backing up" stays on screen at least, so a write that takes no time is still seen. */
export const RUN_MIN_MS = 700;
/** How long the check mark of a finished write stays. */
export const DONE_MS = 1800;

/** How much of the least time is left for a write first seen at `since`. */
export function holdLeft(since: number | null, now: number): number {
  return since === null ? 0 : Math.min(RUN_MIN_MS, Math.max(0, since + RUN_MIN_MS - now));
}

/** What a write to the folder added or changed, as one line. `c` names the kinds the settings file holds. */
export function writtenLine(r: WriteReport, c: BackupCategories): string {
  const parts: string[] = [];
  if (r.rows) parts.push(t("backup.wrote_rows", { n: n(r.rows) }));
  if (r.cells) parts.push(t("backup.wrote_cells", { n: n(r.cells) }));
  if (r.recordings) parts.push(t("backup.wrote_recs", { n: n(r.recordings) }));
  if (r.settings && c.settings) parts.push(t("backup.wrote_settings"));
  if (r.settings && c.content) parts.push(t("backup.wrote_content"));
  return parts.length ? t("backup.wrote", { parts: parts.join(t("backup.list_sep")) }) : t("backup.wrote_none");
}

/** `folder` is null until the page has looked at the folder once; the row then claims nothing about it. */
export function statusView(s: BackupStatus, time: (ts: number) => string, c: BackupCategories, folder: FolderState | null = "usable"): StatusView {
  if (folder === null && s.state !== "running") return { tone: "idle", title: t("backup.status_checking"), detail: "", fix: null, run: false };
  if (folder !== null && folder !== "usable" && s.state !== "running") {
    // The folder as it is now comes before what the last write said.
    const last = s.state === "ok" ? s.at : s.state === "failed" ? s.last_ok : null;
    const detail = t(`backup.folder_${folder}_detail`);
    return { tone: "bad", title: t(`backup.folder_${folder}`), detail: last == null ? detail : t("backup.folder_detail_last", { detail, time: time(last) }), fix: FOLDER_FIX[folder], run: folder !== "missing" };
  }
  switch (s.state) {
    case "never": return { tone: "idle", title: t("backup.status_never"), detail: t("backup.status_never_detail"), fix: null, run: true };
    case "ok": return { tone: "ok", title: t("backup.status_ok"), detail: [time(s.at), ...(s.report.recreated ? [t("backup.recreated")] : []), writtenLine(s.report, c)].join(" · "), fix: null, run: true };
    case "running": return { tone: "busy", title: t("backup.status_running"), detail: "", fix: null, run: true };
    case "failed": {
      // A write the folder refused is said in the interface language; other reasons come as the backend gives them.
      const reason = s.folder && s.folder !== "usable" ? t(`backup.failed_${s.folder}`) : s.reason;
      return { tone: "bad", title: t("backup.status_failed"), detail: s.last_ok == null ? reason : t("backup.status_failed_last", { reason, time: time(s.last_ok) }), fix: null, run: true };
    }
  }
}

export interface ResultLine { tone: "ok" | "warn"; text: string; detail?: string }

/** What importing a backup did. `retentionDays` is the history retention (0 keeps everything). */
export function importLines(r: ImportReport, retentionDays: number): ResultLine[] {
  const lines: ResultLine[] = [{ tone: "ok", text: t("backup.ib_added", { added: n(r.added), updated: n(r.updated), skipped: n(r.skipped) }) }];
  lines.push({ tone: "ok", text: t("backup.ib_cells", { n: n(r.cells) }) });
  if (r.recordings) lines.push({ tone: "ok", text: t("backup.ib_recs", { n: n(r.recordings) }) });
  if (r.missing_recordings) lines.push({ tone: "warn", text: t("backup.ib_missing", { n: n(r.missing_recordings) }) });
  if (r.bad_lines) lines.push({ tone: "warn", text: t("backup.ib_bad", { n: n(r.bad_lines) }) });
  if (r.expiring && retentionDays > 0) lines.push({ tone: "warn", text: t("backup.ib_expiring", { days: n(retentionDays) }), detail: t("backup.ib_expiring_detail", { n: n(r.expiring) }) });
  return lines;
}
