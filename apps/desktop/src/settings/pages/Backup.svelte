<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { backupFolderCreate, backupFolderState, backupRunNow, backupSuggestedFolder, backupStatus, errorMessage, listDevices, onBackupFolder, onBackupStatus, onHistoryChanged, openBackupFolder, openPrivacyPane, removeDevice, renameDevice, type BackupStatus, type ClientSettings, type DeviceRow, type FolderState, type SuggestedFolder } from "../../lib/api";
  import { DONE_MS, FOLDER_LOOK_MS, holdLeft, itemsSummary, statusView } from "../../lib/backupView";
  import { formatDateTime } from "../../lib/format";
  import { t } from "../../lib/i18n";
  import { isImeKey } from "../../lib/ime";
  import { guard } from "../../lib/guard.svelte";
  import { client } from "../../lib/store.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import PageLink from "../../lib/ui/PageLink.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import SaveBar from "../../lib/ui/SaveBar.svelte";
  import Switch from "../../lib/ui/Switch.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import { toast } from "../../lib/ui/toast.svelte";
  import FileDialogs from "./backup/FileDialogs.svelte";
  import RecordDialogs from "./backup/RecordDialogs.svelte";

  type BackupSettings = ClientSettings["backup"];

  let fileDialogs: FileDialogs | undefined = $state();
  let recordDialogs: RecordDialogs | undefined = $state();

  let status = $state<BackupStatus>({ state: "never" });
  // The status before the write that is running: its second line stays, so the row keeps its height.
  let before = $state<BackupStatus>({ state: "never" });
  // What the folder is right now, apart from what the last write said; null until the first look has answered.
  let folder = $state<FolderState | null>(null);
  // Where backups go while no folder is chosen.
  let suggested = $state<SuggestedFolder | null>(null);
  let devices = $state<DeviceRow[]>([]);
  let renaming = $state<string | null>(null);
  let name = $state("");
  let removing = $state<DeviceRow | null>(null);
  // A write that just ended well shows a check mark for a moment; `settled` is the dot coming back after it.
  let fresh = $state(false);
  let settled = $state(false);
  let runningSince: number | null = null;
  let held: ReturnType<typeof setTimeout> | undefined;
  let freshTimer: ReturnType<typeof setTimeout> | undefined;

  // The folder settings as saved, and the draft the page edits; they take effect on Save.
  const saved = $derived(client.value?.backup ?? null);
  let backup = $state<BackupSettings | null>(null);
  let base = "";
  let saving = $state(false);
  // Compared by value: the dialogs and the backend may list the items in another order.
  const canon = (b: BackupSettings | null) => b && JSON.stringify({ folder: b.folder ?? null, auto: b.auto, recreate: b.recreate, categories: Object.fromEntries(Object.entries(b.categories).sort()) });
  const dirty = $derived(backup !== null && canon(backup) !== canon(saved));
  // A folder in the draft that is not the saved one: what the page knows about the saved folder does not apply to it.
  const otherFolder = $derived(!!backup && !!saved && (backup.folder ?? null) !== (saved.folder ?? null));
  const path = $derived(backup?.folder ?? suggested?.path ?? "");
  const kinds = $derived(saved?.categories ?? { settings: false, content: false, history: false, stats: false, recordings: false });
  const view = $derived(statusView(status, formatDateTime, kinds, folder));
  // While a write runs or the folder has not been looked at, the second line keeps its place so the row keeps its height.
  const detail = $derived(status.state === "running" ? statusView(before, formatDateTime, kinds).detail : folder === null ? "\u00a0" : view.detail);
  // The date in file names: the local day as numbers.
  const today = new Date().toLocaleDateString("sv");

  /** Takes a status of the folder. A write stays on screen for a least time, since most end at once. */
  function take(s: BackupStatus) {
    clearTimeout(held);
    if (s.state === "running") {
      runningSince ??= Date.now();
      if (status.state !== "running") before = status;
      clearTimeout(freshTimer);
      fresh = false;
      settled = false;
      status = s;
      return;
    }
    held = setTimeout(() => {
      const ran = runningSince !== null;
      runningSince = null;
      status = s;
      if (!ran || s.state !== "ok") return;
      fresh = true;
      freshTimer = setTimeout(() => { fresh = false; settled = true; }, DONE_MS);
    }, holdLeft(runningSince, Date.now()));
  }

  const loadDevices = () => listDevices().then((d) => { devices = d; }).catch((e) => toast.error(errorMessage(e)));

  /** Looks at the folder. With `probe` a small file is written to it, which is what tells whether it takes one. */
  const look = (probe: boolean) => backupFolderState(probe).then((f) => { folder = f; }).catch(() => {});
  // Coming back to the window, for instance from System Settings, is a moment to try a write again.
  const onFocus = () => look(true);

  onMount(() => {
    loadDevices();
    backupStatus().then(take).catch(() => {});
    backupSuggestedFolder().then((s) => { suggested = s; }).catch(() => {});
    look(true);
    const offStatus = onBackupStatus(take);
    const offFolder = onBackupFolder((f) => { folder = f; });
    const offHistory = onHistoryChanged(loadDevices);
    const looking = setInterval(() => look(false), FOLDER_LOOK_MS);
    window.addEventListener("focus", onFocus);
    return () => { clearTimeout(held); clearTimeout(freshTimer); clearInterval(looking); window.removeEventListener("focus", onFocus); offStatus.then((off) => off()); offFolder.then((off) => off()); offHistory.then((off) => off()); };
  });

  // The draft follows the saved settings while it has no edits of its own.
  $effect(() => {
    const json = JSON.stringify(saved);
    untrack(() => {
      if (saved && (backup === null || canon(backup) === base)) backup = JSON.parse(json);
      base = canon(saved) ?? "";
    });
  });

  $effect(() => {
    guard.set(dirty);
    return () => guard.set(false);
  });

  const revert = () => { if (saved) backup = JSON.parse(JSON.stringify(saved)); };

  async function save() {
    if (!client.value || !backup) return;
    const before = saved;
    saving = true;
    try {
      await client.save({ ...client.value, backup: $state.snapshot(backup) });
      await look(true);
      // Switched on for a folder that is gone: made again now rather than at the next change.
      if (backup.auto && backup.recreate && !(before?.auto && before.recreate) && folder === "missing") updateNow();
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      saving = false;
    }
  }

  async function chooseFolder() {
    const picked = await openDialog({ directory: true, multiple: false, defaultPath: backup?.folder ?? undefined });
    if (typeof picked === "string" && backup) backup.folder = picked;
  }

  async function createFolder() {
    try {
      folder = await backupFolderCreate();
      if (folder !== "usable") toast.error(t("backup.create_failed"));
    } catch (e) { toast.error(errorMessage(e)); }
  }

  const openSystem = () => openPrivacyPane("files").catch((e) => toast.error(errorMessage(e)));

  async function openFolder() {
    try { await openBackupFolder(); } catch (e) { toast.error(errorMessage(e)); }
  }

  async function updateNow() {
    try { await backupRunNow(); } catch (e) { toast.error(errorMessage(e)); }
  }

  function startRename(d: DeviceRow) {
    renaming = d.id;
    name = d.name;
  }

  async function saveName() {
    const id = renaming;
    if (!id || !name.trim()) return;
    try {
      await renameDevice(id, name);
      renaming = null;
      await loadDevices();
    } catch (e) { toast.error(errorMessage(e)); }
  }

  function onNameKey(e: KeyboardEvent) {
    if (isImeKey(e)) return;
    if (e.key === "Enter") { e.preventDefault(); saveName(); }
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); renaming = null; }
  }

  async function remove() {
    const d = removing;
    removing = null;
    if (!d) return;
    try {
      const rows = await removeDevice(d.id);
      toast.ok(t("backup.removed", { n: rows.toLocaleString() }));
      await loadDevices();
    } catch (e) { toast.error(errorMessage(e)); }
  }
</script>

<PageHead title={t("nav.backup")} />

<Group title={t("backup.transfer")} help={t("backup.transfer_note")} info={t("backup.transfer_info")}>
  <Row label={t("backup.kind.settings")} help={t("backup.kind.settings_help")}>
    <span class="buttons">
      <button type="button" class="btn" onclick={() => fileDialogs?.openExport("settings")}>{t("backup.export")}</button>
      <button type="button" class="btn" onclick={() => fileDialogs?.openImport("settings")}>{t("backup.import")}</button>
    </span>
  </Row>
  <Row label={t("backup.kind.content")} help={t("backup.kind.content_help")}>
    <span class="buttons">
      <button type="button" class="btn" onclick={() => fileDialogs?.openExport("content")}>{t("backup.export")}</button>
      <button type="button" class="btn" onclick={() => fileDialogs?.openImport("content")}>{t("backup.import")}</button>
    </span>
  </Row>
  <Row label={t("backup.kind.records")} help={t("backup.kind.records_help")}>
    <span class="buttons">
      <button type="button" class="btn" onclick={() => recordDialogs?.openExport()}>{t("backup.export")}</button>
      <button type="button" class="btn" onclick={() => recordDialogs?.openImport(false)}>{t("backup.import")}</button>
    </span>
  </Row>
</Group>

{#snippet folderTag()}<Tag tone="live">{t(folder === "missing" ? "backup.folder_tag_missing" : "backup.folder_tag_not_writable")}</Tag>{/snippet}
{#snippet folderPath()}{#if path}{path}{:else}&nbsp;{/if}{/snippet}

{#if backup}
  <Group title={t("backup.folder")} help={t("backup.folder_note")} info={t("backup.folder_info")}>
    <Row label={t("backup.folder_path")} info={t("backup.folder_path_info")} tag={!otherFolder && folder && folder !== "usable" && folder !== "unmade" ? folderTag : undefined} help={folderPath} reset={backup.folder && suggested ? { hint: t("common.reset_to", { value: suggested.short }), run: () => { if (backup) backup.folder = null; } } : undefined}>
      <span class="buttons">
        {#if !otherFolder && view.fix === "create"}
          <button type="button" class="btn primary" onclick={createFolder}>{t("backup.create_again")}</button>
        {:else}
          <button type="button" class="btn quiet" disabled={otherFolder || folder === "unmade"} onclick={openFolder}>{t("backup.folder_open")}</button>
        {/if}
        {#if !otherFolder && view.fix === "system"}<button type="button" class="btn" onclick={openSystem}>{t("backup.open_system_settings")}</button>{/if}
        <button type="button" class="btn" onclick={chooseFolder}>{t("backup.folder_choose")}</button>
      </span>
    </Row>
    <Row label={t("backup.items")} help={itemsSummary(backup.categories)}>
      <button type="button" class="btn" onclick={() => backup && recordDialogs?.openItems(backup.categories, (c) => { if (backup) backup.categories = c; })}>{t("backup.items_change")}</button>
    </Row>
    <Row label={t("backup.auto")} help={t(backup.auto ? "backup.auto_help" : "backup.auto_off")} info={backup.auto ? t("backup.auto_info") : undefined}>
      <Switch checked={backup.auto} label={t("backup.auto")} onchange={(on) => { if (backup) backup.auto = on; }} />
    </Row>
    <Row label={t("backup.recreate")} help={t(backup.recreate ? "backup.recreate_on" : "backup.recreate_off")}>
      <Switch checked={backup.recreate} disabled={!backup.auto} label={t("backup.recreate")} onchange={(on) => { if (backup) backup.recreate = on; }} />
    </Row>
    <Row>
      {#snippet leading()}
        <span class="mark">
          {#if status.state === "running"}
            <span class="spin"></span>
          {:else if fresh}
            <span class="done"><svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 6.4l2.4 2.4 4.6-5.2" /></svg></span>
          {:else}
            <span class="dot {view.tone}" class:settled></span>
          {/if}
        </span>
      {/snippet}
      <span class="status">
        <span class="title" class:bad={view.tone === "bad"}>{view.title}</span>
        {#if detail}<small class:fresh>{detail}</small>{/if}
      </span>
      <button type="button" class="btn" disabled={dirty || status.state === "running" || !view.run} onclick={updateNow}>{t("backup.now")}</button>
    </Row>
    <Row label={t("backup.import_folder")} help={t("backup.import_folder_help")}>
      <button type="button" class="btn" onclick={() => recordDialogs?.openImport(true, folder === "unmade" ? null : path)}>{t("backup.choose_folder")}</button>
    </Row>
  </Group>
{/if}

{#snippet devicesInfo()}{t("backup.devices_info_before").slice(0, -1)}<span class="nobr">{t("backup.devices_info_before").slice(-1)}<PageLink page="home" label={t("nav.home")} />{t("backup.devices_info_mid")}<PageLink page="history" label={t("nav.history")} />{t("backup.devices_info_after")}</span>{/snippet}
<Group title={t("backup.devices")} help={t("backup.devices_note")} info={devicesInfo}>
  {#each devices as d (d.id)}
    <Row>
      {#snippet leading()}<span class="device"><Icon name="device" size={18} /></span>{/snippet}
      {#if renaming === d.id}
        <!-- svelte-ignore a11y_autofocus -->
        <input class="rename" type="text" bind:value={name} maxlength="80" aria-label={t("backup.device_name")} onkeydown={onNameKey} autofocus />
        <span class="buttons">
          <button type="button" class="btn" onclick={() => (renaming = null)}>{t("common.cancel")}</button>
          <button type="button" class="btn primary" disabled={!name.trim()} onclick={saveName}>{t("common.save")}</button>
        </span>
      {:else}
        <span class="status">
          <span class="title">{d.name}{#if d.this}&nbsp;<Tag tone="accent">{t("backup.device_this")}</Tag>{/if}</span>
          <small>{d.rows && d.last_ts != null ? t("backup.device_rows", { n: d.rows.toLocaleString(), time: formatDateTime(d.last_ts) }) : t("backup.device_empty")}</small>
        </span>
        <span class="buttons">
          <button type="button" class="btn" onclick={() => startRename(d)}>{t("backup.rename")}</button>
          {#if !d.this}<button type="button" class="btn danger" onclick={() => (removing = d)}>{t("backup.remove")}</button>{/if}
        </span>
      {/if}
    </Row>
  {/each}
</Group>

<FileDialogs bind:this={fileDialogs} {today} />
<RecordDialogs bind:this={recordDialogs} {today} onchanged={loadDevices} />

<Modal open={removing !== null} title={t("backup.remove_title", { name: removing?.name ?? "" })} onclose={() => (removing = null)}>
  <p>{t("backup.remove_body", { name: removing?.name ?? "", n: (removing?.rows ?? 0).toLocaleString() })}</p>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (removing = null)}>{t("common.cancel")}</button>
    <button type="button" class="btn danger" onclick={remove}>{t("backup.remove_confirm")}</button>
  {/snippet}
</Modal>

<SaveBar {dirty} {saving} onsave={save} onrevert={revert} />

<style>
  .buttons { display: flex; align-items: center; gap: 8px; flex: none; }
  .status { flex: 1; min-width: 0; }
  .title { display: block; font-weight: 600; overflow-wrap: anywhere; }
  .title.bad { color: var(--live); }
  .status small { display: block; margin-top: 2px; color: var(--ink-2); font-size: var(--text-sm); overflow-wrap: anywhere; }
  .dot { width: 9px; height: 9px; border-radius: 50%; flex: none; background: var(--ink-3); }
  .dot.ok { background: var(--ok); }
  .dot.bad { background: var(--live); }
  .dot.settled { animation: settle .3s; }
  .mark { width: 18px; height: 18px; flex: none; display: grid; place-content: center; }
  .spin { width: 16px; height: 16px; border-radius: 50%; border: 2.5px solid var(--accent-soft); border-top-color: var(--accent); animation: turn .7s linear infinite; }
  .done { width: 18px; height: 18px; border-radius: 50%; background: var(--ok); display: grid; place-content: center; animation: pop .32s cubic-bezier(.2, .9, .3, 1.4); }
  .done svg { width: 12px; height: 12px; }
  .done path { fill: none; stroke: var(--surface); stroke-width: 2.4; stroke-linecap: round; stroke-linejoin: round; stroke-dasharray: 16; stroke-dashoffset: 16; animation: draw .26s .12s ease-out forwards; }
  .status small { transition: color .3s; }
  .status small.fresh { color: var(--ok); }
  @keyframes turn { to { transform: rotate(360deg); } }
  @keyframes pop { from { transform: scale(.4); opacity: 0; } }
  @keyframes draw { to { stroke-dashoffset: 0; } }
  @keyframes settle { from { transform: scale(.6); opacity: 0; } }
  @media (prefers-reduced-motion: reduce) {
    .spin, .done, .dot.settled { animation: none; }
    .done path { animation: none; stroke-dashoffset: 0; }
  }
  .device { display: grid; place-content: center; width: 36px; height: 36px; border-radius: var(--radius); background: var(--sunken); color: var(--ink-2); flex: none; }
  .rename { flex: 1; min-width: 0; }
</style>
