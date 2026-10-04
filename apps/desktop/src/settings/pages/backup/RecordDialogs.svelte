<script lang="ts">
  import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { backupSizes, errorMessage, exportBackup, formatBytes, importBackup, inspectBackup, type BackupCategories, type BackupInfo, type BackupSizes, type ImportReport } from "../../../lib/api";
  import { deviceLine, importLines } from "../../../lib/backupView";
  import { t } from "../../../lib/i18n";
  import { client, config } from "../../../lib/store.svelte";
  import Modal from "../../../lib/ui/Modal.svelte";
  import { toast } from "../../../lib/ui/toast.svelte";
  import Checks, { type CheckItem } from "./Checks.svelte";
  import Result from "./Result.svelte";

  let { today, onchanged }: { today: string; onchanged: () => void } = $props();

  let mode = $state<"export" | "folder" | "import" | "result" | null>(null);
  let cats = $state<Record<string, boolean>>({});
  let devices = $state<Record<string, boolean>>({});
  let busy = $state(false);
  let sizes = $state<BackupSizes | null>(null);
  let source = { path: "", folder: false };
  let sourceName = $state("");
  let info = $state<BackupInfo | null>(null);
  let report = $state<ImportReport | null>(null);

  const asCategories = (c: Record<string, boolean>): BackupCategories => ({ settings: !!c.settings, content: !!c.content, history: !!c.history, stats: !!c.stats, recordings: !!c.recordings });
  const count = (v: number) => v.toLocaleString();

  /** The three kinds of records, with what this device has of each. */
  const recordItems = $derived<CheckItem[]>([
    { key: "history", label: t("backup.cat.history"), help: sizes ? t("backup.cat.history_help", { n: count(sizes.rows) }) : undefined },
    { key: "stats", label: t("backup.cat.stats"), help: t("backup.cat.stats_help") },
    {
      key: "recordings", label: t("backup.cat.recordings"),
      help: sizes ? t("backup.cat.recordings_help", { n: count(sizes.recordings), size: formatBytes(sizes.recording_bytes) }) : undefined,
      warn: t("backup.cat.recordings_warn"),
    },
  ]);
  const settingsItems = $derived<CheckItem[]>([{ key: "settings", label: t("backup.cat.settings"), help: t("backup.cat.settings_help") }]);
  const contentItems = $derived<CheckItem[]>([{ key: "content", label: t("backup.cat.content"), help: t("backup.cat.content_help") }]);

  const picked = $derived((info?.devices ?? []).filter((d) => devices[d.id]));
  const incoming = $derived({
    rows: picked.reduce((sum, d) => sum + d.rows, 0),
    recordings: picked.reduce((sum, d) => sum + d.recordings, 0),
    bytes: picked.reduce((sum, d) => sum + d.recording_bytes, 0),
  });
  const deviceItems = $derived<CheckItem[]>((info?.devices ?? []).map((d) => ({ key: d.id, label: d.this ? `${d.name} (${t("backup.device_this")})` : d.name, help: deviceLine(d) })));
  const incomingItems = $derived<CheckItem[]>([
    { key: "history", label: t("backup.cat.history"), help: t("backup.rows", { n: count(incoming.rows) }) },
    { key: "stats", label: t("backup.cat.stats") },
    {
      key: "recordings", label: t("backup.cat.recordings"), disabled: !incoming.recordings || !cats.history,
      help: incoming.recordings ? t("backup.cat.recordings_help", { n: count(incoming.recordings), size: formatBytes(incoming.bytes) }) : t("backup.none_there"),
      warn: incoming.recordings ? t("backup.cat.recordings_warn") : undefined,
    },
  ]);

  const loadSizes = () => backupSizes().then((s) => { sizes = s; }).catch(() => { sizes = null; });

  export function openExport() {
    cats = { history: true, stats: true, recordings: false };
    loadSizes();
    mode = "export";
  }

  export function openItems() {
    cats = { ...(client.value?.backup.categories ?? {}) };
    loadSizes();
    mode = "folder";
  }

  /** `start` is where the folder dialog opens. */
  export async function openImport(folder: boolean, start?: string | null) {
    try {
      const chosen = await openDialog(folder ? { directory: true, multiple: false, defaultPath: start ?? undefined } : { multiple: false, directory: false, filters: [{ name: "Arcvox", extensions: ["arcvox"] }] });
      if (typeof chosen !== "string") return;
      busy = true;
      info = await inspectBackup(chosen, folder);
      source = { path: chosen, folder };
      sourceName = chosen.split(/[\\/]/).pop() ?? chosen;
      devices = Object.fromEntries(info.devices.map((d) => [d.id, true]));
      cats = { history: true, stats: true, recordings: false };
      mode = "import";
    } catch (e) { toast.error(errorMessage(e)); }
    busy = false;
  }

  async function doExport() {
    try {
      const target = await saveDialog({ defaultPath: `arcvox-records-${today}.arcvox`, filters: [{ name: "Arcvox", extensions: ["arcvox"] }] });
      if (typeof target !== "string") return;
      busy = true;
      await exportBackup(target, asCategories(cats));
      mode = null;
      toast.ok(t("backup.exported.records"));
    } catch (e) { toast.error(errorMessage(e)); }
    busy = false;
  }

  async function saveItems() {
    if (!client.value) return;
    try {
      await client.save({ ...client.value, backup: { ...client.value.backup, categories: asCategories(cats) } });
      mode = null;
    } catch (e) { toast.error(errorMessage(e)); }
  }

  async function doImport() {
    busy = true;
    try {
      const chosen = asCategories(cats);
      report = await importBackup(source.path, source.folder, picked.map((d) => d.id), { ...chosen, recordings: chosen.recordings && chosen.history });
      mode = "result";
      onchanged();
    } catch (e) { toast.error(errorMessage(e)); }
    busy = false;
  }

  const anyRecords = $derived(!!(cats.history || cats.stats));
</script>

<Modal open={mode === "export"} title={t("backup.export_title.records")} onclose={() => (mode = null)}>
  <p class="lead">{t("backup.export_lead.records")}</p>
  <Checks items={recordItems} bind:value={cats} />
  <p class="callout">{t("backup.export_note.records")}</p>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (mode = null)}>{t("common.cancel")}</button>
    <button type="button" class="btn primary" disabled={busy || !anyRecords} onclick={doExport}>{busy ? t("backup.working") : t("backup.export")}</button>
  {/snippet}
</Modal>

<Modal open={mode === "folder"} title={t("backup.items_title")} onclose={() => (mode = null)}>
  <p class="lead">{t("backup.items_lead")}</p>
  <div class="head first">{t("backup.kind.settings")}</div>
  <Checks items={settingsItems} bind:value={cats} />
  <div class="head">{t("backup.kind.content")}</div>
  <Checks items={contentItems} bind:value={cats} />
  <div class="head">{t("backup.kind.records")}</div>
  <Checks items={recordItems} bind:value={cats} />
  {#if cats.recordings}<p class="callout warn">{t("backup.items_warn")}</p>{/if}
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (mode = null)}>{t("common.cancel")}</button>
    <button type="button" class="btn primary" onclick={saveItems}>{t("backup.done")}</button>
  {/snippet}
</Modal>

<Modal open={mode === "import"} title={t("backup.import_title.records")} onclose={() => (mode = null)}>
  <p class="lead">{t("backup.import_lead.records")}</p>
  <dl class="meta"><dt>{t("backup.source")}</dt><dd>{sourceName}</dd></dl>
  <div class="head">{t("backup.records_devices")}</div>
  {#if deviceItems.length}<Checks items={deviceItems} bind:value={devices} />{:else}<p class="callout">{t("backup.records_none")}</p>{/if}
  <div class="head">{t("backup.items")}</div>
  <Checks items={incomingItems} bind:value={cats} />
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (mode = null)}>{t("common.cancel")}</button>
    <button type="button" class="btn primary" disabled={busy || !picked.length || !anyRecords} onclick={doImport}>{busy ? t("backup.working") : t("backup.apply")}</button>
  {/snippet}
</Modal>

<Modal open={mode === "result"} title={t("backup.imported_title.records")} onclose={() => (mode = null)}>
  <p class="lead">{t("backup.records_done_lead", { n: String(report?.devices ?? 0) })}</p>
  {#if report}<Result lines={importLines(report, config.value?.general.history_retention_days ?? 0)} />{/if}
  {#snippet actions()}
    <button type="button" class="btn primary" onclick={() => (mode = null)}>{t("backup.done")}</button>
  {/snippet}
</Modal>

<style>
  .lead { margin: 0 0 14px; font-size: var(--text-control); }
  .head { margin: 14px 2px 8px; font-size: var(--text-sm); font-weight: 600; color: var(--ink); }
  .head.first { margin-top: 0; }
  .callout { margin: 12px 0 0; padding: 10px 12px; border-radius: 10px; background: var(--sunken); font-size: var(--text-sm); }
  .callout.warn { color: var(--warn); }
  .meta { display: grid; grid-template-columns: auto 1fr; gap: 2px 14px; margin: 0; padding: 10px 14px; border-radius: 10px; background: var(--sunken); font-size: var(--text-sm); }
  .meta dd { margin: 0; color: var(--ink); font-weight: 600; overflow-wrap: anywhere; }
</style>
