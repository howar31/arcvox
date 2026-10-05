<script lang="ts">
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { backupFolderCreate, backupFolderState, backupRunNow, backupStatus, errorMessage, listDevices, onBackupFolder, onBackupStatus, onHistoryChanged, openBackupFolder, openPrivacyPane, removeDevice, renameDevice, type BackupStatus, type ClientSettings, type DeviceRow, type FolderState } from "../../lib/api";
  import { DONE_MS, FOLDER_LOOK_MS, holdLeft, itemsSummary, statusView } from "../../lib/backupView";
  import { formatDateTime } from "../../lib/format";
  import { t } from "../../lib/i18n";
  import { isImeKey } from "../../lib/ime";
  import { client } from "../../lib/store.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Switch from "../../lib/ui/Switch.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import { toast } from "../../lib/ui/toast.svelte";
  import FileDialogs from "./backup/FileDialogs.svelte";
  import RecordDialogs from "./backup/RecordDialogs.svelte";

  let fileDialogs: FileDialogs | undefined = $state();
  let recordDialogs: RecordDialogs | undefined = $state();

  let status = $state<BackupStatus>({ state: "never" });
  // The status before the write that is running: its second line stays, so the row keeps its height.
  let before = $state<BackupStatus>({ state: "never" });
  // What the folder is right now, apart from what the last write said.
  let folder = $state<FolderState>("usable");
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

  const backup = $derived(client.value?.backup ?? null);
  const kinds = $derived(backup?.categories ?? { settings: false, content: false, history: false, stats: false, recordings: false });
  const view = $derived(statusView(status, formatDateTime, kinds, folder));
  const detail = $derived(status.state === "running" ? statusView(before, formatDateTime, kinds).detail : view.detail);
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
    look(true);
    const offStatus = onBackupStatus(take);
    const offFolder = onBackupFolder((f) => { folder = f; });
    const offHistory = onHistoryChanged(loadDevices);
    const looking = setInterval(() => look(false), FOLDER_LOOK_MS);
    window.addEventListener("focus", onFocus);
    return () => { clearTimeout(held); clearTimeout(freshTimer); clearInterval(looking); window.removeEventListener("focus", onFocus); offStatus.then((off) => off()); offFolder.then((off) => off()); offHistory.then((off) => off()); };
  });

  /** The backup settings take effect at once; this page has no save bar. */
  async function saveBackup(change: Partial<ClientSettings["backup"]>) {
    if (!client.value) return;
    try { await client.save({ ...client.value, backup: { ...client.value.backup, ...change } }); } catch (e) { toast.error(errorMessage(e)); }
  }

  async function chooseFolder() {
    const picked = await openDialog({ directory: true, multiple: false, defaultPath: backup?.folder ?? undefined });
    if (typeof picked === "string") { await saveBackup({ folder: picked }); await look(true); }
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

<Group title={t("backup.transfer")} help={t("backup.transfer_note")}>
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

{#if backup}
  <Group title={t("backup.folder")} help={t("backup.folder_note")}>
    <Row label={t("backup.folder_path")} tag={backup.folder && folder !== "usable" ? folderTag : undefined} help={backup.folder ?? t("backup.folder_none")}>
      <span class="buttons">
        {#if backup.folder && view.fix === "create"}
          <button type="button" class="btn primary" onclick={createFolder}>{t("backup.create_again")}</button>
        {:else}
          <button type="button" class="btn quiet" disabled={!backup.folder} onclick={openFolder}>{t("backup.folder_open")}</button>
        {/if}
        {#if backup.folder && view.fix === "system"}<button type="button" class="btn" onclick={openSystem}>{t("backup.open_system_settings")}</button>{/if}
        <button type="button" class="btn" onclick={chooseFolder}>{t("backup.folder_choose")}</button>
      </span>
    </Row>
    <Row label={t("backup.items")} help={itemsSummary(backup.categories)}>
      <button type="button" class="btn" onclick={() => recordDialogs?.openItems()}>{t("backup.items_change")}</button>
    </Row>
    <Row label={t("backup.auto")} help={t("backup.auto_help")}>
      <Switch checked={backup.auto} disabled={!backup.folder} label={t("backup.auto")} onchange={(on) => saveBackup({ auto: on })} />
    </Row>
    <Row label={t("backup.recreate")} help={t("backup.recreate_help")}>
      <Switch checked={backup.recreate} disabled={!backup.folder || !backup.auto} label={t("backup.recreate")} onchange={(on) => saveBackup({ recreate: on }).then(() => { if (on && folder === "missing") updateNow(); })} />
    </Row>
    {#if backup.folder}
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
        <button type="button" class="btn" disabled={status.state === "running" || !view.run} onclick={updateNow}>{t("backup.now")}</button>
      </Row>
    {/if}
    <Row label={t("backup.import_folder")} help={t("backup.import_folder_help")}>
      <button type="button" class="btn" onclick={() => recordDialogs?.openImport(true, backup.folder)}>{t("backup.choose_folder")}</button>
    </Row>
  </Group>
{/if}

<Group title={t("backup.devices")} help={t("backup.devices_note")}>
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
