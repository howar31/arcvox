<script lang="ts">
  import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { errorMessage, exportSettings, importSettings, inspectSettings, type SettingsInfo, type SettingsResult } from "../../../lib/api";
  import { KIND_ITEMS, noteLines, platformName, type FileKind } from "../../../lib/backupView";
  import { formatDateTime } from "../../../lib/format";
  import { t } from "../../../lib/i18n";
  import { go } from "../../../lib/route";
  import { client, config } from "../../../lib/store.svelte";
  import Modal from "../../../lib/ui/Modal.svelte";
  import { toast } from "../../../lib/ui/toast.svelte";
  import Checks, { type CheckItem } from "./Checks.svelte";
  import Result, { type Line } from "./Result.svelte";

  let { today }: { today: string } = $props();

  // Settings and content are files of the same form with different items.
  let kind = $state<FileKind>("settings");
  const items = $derived<readonly string[]>(KIND_ITEMS[kind]);
  let mode = $state<"export" | "import" | "result" | null>(null);
  let picked = $state<Record<string, boolean>>({});
  let busy = $state(false);
  let path = "";
  let info = $state<SettingsInfo | null>(null);
  let result = $state<SettingsResult | null>(null);

  const every = (on: boolean) => Object.fromEntries(items.map((s) => [s, on]));
  const help = (s: string) => (s === "base_prompt" ? undefined : t(`backup.section.${s}_help`));
  const exportItems = $derived<CheckItem[]>(items.map((s) => ({ key: s, label: t(`backup.section.${s}`), help: help(s) })));
  const importItems = $derived<CheckItem[]>(items.map((s) => {
    const present = info?.sections.includes(s) ?? false;
    return { key: s, label: t(`backup.section.${s}`), help: present ? help(s) : t("backup.section_missing"), disabled: !present };
  }));
  const chosen = $derived(items.filter((s) => picked[s] && (mode !== "import" || info?.sections.includes(s))));
  const allOn = $derived(items.every((s) => picked[s]));

  export function openExport(of: FileKind) {
    kind = of;
    picked = every(true);
    mode = "export";
  }

  export async function openImport(of: FileKind) {
    try {
      const chosenFile = await openDialog({ multiple: false, directory: false, filters: [{ name: "Arcvox", extensions: ["json"] }] });
      if (typeof chosenFile !== "string") return;
      const found = await inspectSettings(chosenFile);
      // A file of the other kind has nothing for this dialog.
      if (!KIND_ITEMS[of].some((s) => found.sections.includes(s))) { toast.error(t(`backup.import_empty.${of}`)); return; }
      kind = of;
      path = chosenFile;
      info = found;
      picked = every(true);
      mode = "import";
    } catch (e) { toast.error(errorMessage(e)); }
  }

  async function doExport() {
    try {
      const target = await saveDialog({ defaultPath: `arcvox-${kind}-${today}.json`, filters: [{ name: "Arcvox", extensions: ["json"] }] });
      if (typeof target !== "string") return;
      busy = true;
      await exportSettings(target, [...chosen]);
      mode = null;
      toast.ok(t(`backup.exported.${kind}`));
    } catch (e) { toast.error(errorMessage(e)); }
    busy = false;
  }

  async function doImport() {
    busy = true;
    try {
      result = await importSettings(path, [...chosen]);
      // The pages read the settings from these stores.
      await Promise.all([config.load(), client.load()]);
      mode = "result";
    } catch (e) { toast.error(errorMessage(e)); }
    busy = false;
  }

  const lines = $derived.by<Line[]>(() => {
    if (!result) return [];
    const applied: Line[] = result.applied.length
      ? [{ tone: "ok", text: result.applied.map((s) => t(`backup.section.${s}`)).join(t("backup.list_sep")) }]
      : [{ tone: "warn", text: t("backup.imported_none") }];
    return [...applied, ...noteLines(result.notes).map((l): Line => ({
      tone: "warn", text: l.text, detail: l.detail,
      action: l.go ? { label: t("backup.note.go_engines"), run: () => { mode = null; go(l.go!); } } : undefined,
    }))];
  });
</script>

<Modal open={mode === "export"} title={t(`backup.export_title.${kind}`)} onclose={() => (mode = null)}>
  <p class="lead">{t(`backup.export_lead.${kind}`)}</p>
  <div class="head"><span>{t("backup.items")}</span><button type="button" class="btn quiet" onclick={() => (picked = every(!allOn))}>{t(allOn ? "backup.select_none" : "backup.select_all")}</button></div>
  <Checks items={exportItems} bind:value={picked} />
  <p class="callout">{t(`backup.export_note.${kind}`)}</p>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (mode = null)}>{t("common.cancel")}</button>
    <button type="button" class="btn primary" disabled={busy || !chosen.length} onclick={doExport}>{t("backup.export")}</button>
  {/snippet}
</Modal>

<Modal open={mode === "import"} title={t(`backup.import_title.${kind}`)} onclose={() => (mode = null)}>
  <p class="lead">{t(`backup.import_lead.${kind}`)}</p>
  {#if info}
    <dl class="meta">
      <dt>{t("backup.file")}</dt><dd>{info.file_name}</dd>
      <dt>{t("backup.source")}</dt><dd>{t("backup.source_value", { version: info.app_version, platform: platformName(info.platform) })}</dd>
      <dt>{t("backup.exported_at")}</dt><dd>{formatDateTime(info.exported_at)}</dd>
    </dl>
  {/if}
  <div class="head"><span>{t("backup.in_file")}</span></div>
  <Checks items={importItems} bind:value={picked} />
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (mode = null)}>{t("common.cancel")}</button>
    <button type="button" class="btn primary" disabled={busy || !chosen.length} onclick={doImport}>{busy ? t("backup.working") : t("backup.apply")}</button>
  {/snippet}
</Modal>

<Modal open={mode === "result"} title={t(`backup.imported_title.${kind}`)} onclose={() => (mode = null)}>
  <p class="lead">{t("backup.imported_lead", { n: String(result?.applied.length ?? 0) })}</p>
  <Result {lines} />
  {#snippet actions()}
    <button type="button" class="btn primary" onclick={() => (mode = null)}>{t("backup.done")}</button>
  {/snippet}
</Modal>

<style>
  .lead { margin: 0 0 14px; font-size: var(--text-control); }
  .head { display: flex; align-items: center; justify-content: space-between; min-height: 24px; margin: 14px 2px 8px; font-size: var(--text-sm); font-weight: 600; }
  .head .btn { height: 24px; }
  .callout { margin: 12px 0 0; padding: 10px 12px; border-radius: 10px; background: var(--sunken); font-size: var(--text-sm); }
  .meta { display: grid; grid-template-columns: auto 1fr; gap: 2px 14px; margin: 0; padding: 10px 14px; border-radius: 10px; background: var(--sunken); font-size: var(--text-sm); }
  .meta dd { margin: 0; color: var(--ink); font-weight: 600; overflow-wrap: anywhere; }
</style>
