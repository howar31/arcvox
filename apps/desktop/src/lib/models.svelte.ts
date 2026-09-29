import type { DownloadProgress, InstalledModel, ModelEntry } from "./api";

export type RowStatus =
  | { kind: "missing" }
  | { kind: "downloading"; received: number; total: number }
  | { kind: "installed"; size: number; custom: boolean }
  | { kind: "error"; message: string };

export function rowStatus(entry: ModelEntry, installed: InstalledModel[], progress: Record<string, DownloadProgress>): RowStatus {
  const file = installed.find((m) => m.id === entry.id);
  if (file) return { kind: "installed", size: file.size_bytes, custom: file.custom };
  const p = progress[entry.id];
  if (p && !p.done) return { kind: "downloading", received: p.received, total: p.total };
  if (p?.error) return { kind: "error", message: p.error };
  return { kind: "missing" };
}
