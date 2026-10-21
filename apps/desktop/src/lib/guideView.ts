import type { DownloadProgress, InstalledModel, ManifestView, ModelEntry, NativeStatusKind, Permissions } from "./api";
import type { Role } from "./engines";

/** The models the first-launch guide offers for each part, in the order shown; the first is ticked. */
export const GUIDE_MODELS: Record<Role, readonly string[]> = {
  asr: ["breeze-asr-25-q5_0", "qwen3-asr-1.7b-q8_0"],
  llm: ["gemma-3-4b-it-q4_k_m", "qwen3-8b-q4_k_m"],
};

/** The engines of the Apple path, by role. */
export const APPLE_ENGINES: Record<Role, string> = { asr: "apple-dictation", llm: "apple-fm" };

/** What the guide does about engines: download the two models, use the Apple services, or leave it for the Engines page. */
export type EngineChoice = "models" | "apple" | "later";

/** How the Apple path stands on this machine: none hides it. */
export type ApplePath = "ready" | "off" | "preparing" | "none";

/** The catalogue entries of a role the guide shows: the offered ones in their order, or every one when `all`. */
export function guideEntries(manifest: ManifestView | null, role: Role, all = false): ModelEntry[] {
  const models = manifest?.models.filter((m) => m.role === role) ?? [];
  if (all) {
    const offered = GUIDE_MODELS[role].map((id) => models.find((m) => m.id === id)).filter((m): m is ModelEntry => !!m);
    return [...offered, ...models.filter((m) => !GUIDE_MODELS[role].includes(m.id))];
  }
  return GUIDE_MODELS[role].map((id) => models.find((m) => m.id === id)).filter((m): m is ModelEntry => !!m);
}

/** A model's download size, its audio encoder included. */
export const entryBytes = (e: ModelEntry): number => e.size_bytes + (e.mmproj?.size_bytes ?? 0);

/** The size of what is still to download: installed models count as done. */
export function bytesToDownload(entries: ModelEntry[], installed: InstalledModel[]): number {
  return entries.filter((e) => !installed.some((m) => m.id === e.id)).reduce((n, e) => n + entryBytes(e), 0);
}

/** Received and total bytes of the chosen downloads, installed models counted as received. */
export function downloadTotals(entries: ModelEntry[], installed: InstalledModel[], progress: Record<string, DownloadProgress>): { received: number; total: number } {
  let received = 0;
  let total = 0;
  for (const e of entries) {
    const size = entryBytes(e);
    total += size;
    if (installed.some((m) => m.id === e.id)) received += size;
    else if (progress[e.id]) received += Math.min(size, progress[e.id].received);
  }
  return { received, total };
}

/** Whether the machine has less memory than the catalogue says the model needs; unknown memory never warns. */
export function memoryShort(entry: ModelEntry, memoryBytes: number): boolean {
  return memoryBytes > 0 && entry.min_ram_gb * 1024 ** 3 > memoryBytes;
}

/** The Apple path from the Foundation Models status; a machine that cannot have it shows no path. */
export function applePath(fm: NativeStatusKind | null): ApplePath {
  switch (fm) {
    case "ready": return "ready";
    case "intelligence_off": return "off";
    case "model_not_ready": return "preparing";
    default: return "none";
  }
}

/** Whether the engine step lets the user go on. */
export function engineStepDone(choice: EngineChoice, apple: ApplePath, started: boolean, remaining: number): boolean {
  if (choice === "later") return true;
  if (choice === "apple") return apple === "ready";
  return started || remaining === 0;
}

/** Whether the permission step lets the user go on: Accessibility granted, the microphone granted or beyond the user's reach. */
export function permissionStepDone(perms: Permissions | null): boolean {
  return !!perms && perms.accessibility && (perms.microphone === "granted" || perms.microphone === "restricted");
}
