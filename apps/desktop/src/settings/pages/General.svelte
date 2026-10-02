<script lang="ts">
  import { onMount } from "svelte";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { clearRecordings, errorMessage, formatBytes, historyStorage, openRecordingsDir, recordingsDir, resetStats, setRecordingsDir, type RecordingsDir, listMicrophones, openPrivacyPane, permissions, requestAccessibility, updateGeneral, widgetPlacing, widgetResetPosition, type ClientSettings, type General, type HistoryStorage, type InsertMethod } from "../../lib/api";
  import { appName, ensureAppNames } from "../../lib/apps.svelte";
  import { t } from "../../lib/i18n";
  import { client, config } from "../../lib/store.svelte";
  import { invalidFields } from "../../lib/numbers";
  import { guard } from "../../lib/guard.svelte";
  import { toast } from "../../lib/ui/toast.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Ring from "../../lib/ui/Ring.svelte";
  import Switch from "../../lib/ui/Switch.svelte";
  import NumberField from "../../lib/ui/NumberField.svelte";
  import ShortcutField from "../../lib/ui/ShortcutField.svelte";
  import { defaultShortcuts, isDefaultShortcuts, MAX_SHORTCUTS } from "../../lib/shortcut";
  import SaveBar from "../../lib/ui/SaveBar.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import AddPerApp from "./AddPerApp.svelte";

  let mics = $state<string[]>([]);
  let accessibility = $state<boolean | null>(null);
  let autostart = $state(false);
  // Drafts are edited in place; the base copies are what is saved on disk.
  let draft = $state<ClientSettings | null>(null);
  let general = $state<General | null>(null);
  let baseClient = $state("");
  let baseGeneral = $state("");
  let saving = $state(false);
  let problem = $state<string | undefined>(undefined);
  // A bundle id chosen or typed but not added yet: counts as a change, and Save adds it first.
  let newApp = $state("");
  // True while the widget is kept on screen so it can be dragged into place.
  let placing = $state(false);

  const clientDirty = $derived(draft !== null && JSON.stringify(draft) !== baseClient);
  const generalDirty = $derived(general !== null && JSON.stringify(general) !== baseGeneral);
  const dirty = $derived(clientDirty || generalDirty || newApp.trim() !== "");
  // The stored device is not connected now: still shown, as the selected option.
  const micAbsent = $derived(!!draft && draft.microphone !== "builtin" && draft.microphone !== "default" && !mics.includes(draft.microphone));

  $effect(() => { if (draft) ensureAppNames(Object.keys(draft.insertion.per_app)); });

  $effect(() => {
    guard.set(dirty);
    return () => guard.set(false);
  });
  $effect(() => {
    // A new edit clears the previous validation message.
    JSON.stringify([draft, general]);
    problem = undefined;
  });

  function reset() {
    newApp = "";
    const c = structuredClone($state.snapshot(client.value!));
    draft = c;
    baseClient = JSON.stringify(c);
    // `config.value` is null when the engine failed to open; the client
    // settings still work since they load independently.
    if (config.value) {
      const g = structuredClone($state.snapshot(config.value.general));
      general = g;
      baseGeneral = JSON.stringify(g);
    }
  }

  async function checkAccess() {
    try { accessibility = (await permissions()).accessibility; } catch { accessibility = null; }
  }

  // Recordings on disk, and the two confirmations of this group.
  let storage = $state<HistoryStorage | null>(null);
  let confirmRecordings = $state(false);
  let confirmStats = $state(false);
  const loadStorage = () => historyStorage().then((s) => { storage = s; }).catch(() => { storage = null; });

  // The folder of the recordings. Changing it moves them at once; it is not part of what Save writes.
  let folder = $state<RecordingsDir | null>(null);
  let moving = $state(false);
  const loadFolder = () => recordingsDir().then((d) => { folder = d; }).catch(() => { folder = null; });

  async function moveRecordings(path: string | null) {
    moving = true;
    try {
      const d = await setRecordingsDir(path);
      folder = d;
      const counts = { n: d.moved.toLocaleString(), left: d.left.toLocaleString() };
      toast.ok(t(d.left ? "general.recordings_dir_left" : "general.recordings_dir_moved", counts));
    } catch (e) { toast.error(errorMessage(e)); }
    moving = false;
    loadStorage();
  }
  async function chooseFolder() {
    const picked = await openDialog({ directory: true, multiple: false, defaultPath: folder?.path });
    if (typeof picked === "string" && picked !== folder?.path) await moveRecordings(picked);
  }

  async function openFolder() {
    try { await openRecordingsDir(); } catch (e) { toast.error(errorMessage(e)); }
  }
  async function clearAllRecordings() {
    confirmRecordings = false;
    try {
      const n = await clearRecordings();
      toast.ok(t("history.recordings_cleared", { n: String(n) }));
    } catch (e) { toast.error(errorMessage(e)); }
    loadStorage();
  }
  async function resetStatistics() {
    confirmStats = false;
    try { await resetStats(); toast.ok(t("general.stats_reset_done")); } catch (e) { toast.error(errorMessage(e)); }
  }

  onMount(() => {
    reset();
    loadStorage();
    loadFolder();
    listMicrophones().then((m) => (mics = m)).catch(() => {});
    checkAccess();
    isEnabled().then((v) => (autostart = v)).catch(() => {});
    const onFocus = () => { checkAccess(); };
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
      if (placing) widgetPlacing(false).catch(() => {});
    };
  });

  function invalid(): string[] {
    const bad: string[] = [];
    if (draft) bad.push(...invalidFields([
      { key: "general.hold_threshold", value: draft.hotkey.hold_threshold_ms, min: 100, max: 1000, integer: true },
      { key: "general.double_tap_window", value: draft.hotkey.double_tap_window_ms, min: 150, max: 1000, integer: true },
      { key: "general.max_utterance", value: draft.max_utterance_secs, min: 10, max: 1800, integer: true },
      { key: "general.auto_stop_secs", value: draft.auto_stop.silence_secs, min: 1, max: 30, integer: true },
    ]));
    if (general) bad.push(...invalidFields([
      { key: "general.history_retention", value: general.history_retention_days, min: 0, integer: true },
      { key: "general.recording_retention", value: general.recording_retention_days, min: 0, integer: true },
      { key: "general.typing_speed", value: general.typing_chars_per_min, min: 1, max: 1000, integer: true },
      { key: "general.asr_timeout", value: general.asr_timeout_secs, min: 1, integer: true },
      { key: "general.llm_timeout", value: general.llm_timeout_secs, min: 1, integer: true },
      { key: "general.retry_count", value: general.retry_count, min: 0, max: 5, integer: true },
      { key: "general.llm_idle_unload", value: general.llm_idle_unload_secs, min: 0, integer: true },
    ]));
    return bad;
  }

  async function save() {
    const bad = invalid();
    if (bad.length) { problem = t("common.invalid_number", { field: t(bad[0]) }); return; }
    if (newApp.trim()) { addApp(newApp.trim()); newApp = ""; }
    saving = true;
    try {
      const languageChanged = clientDirty && draft!.ui_language !== JSON.parse(baseClient).ui_language;
      // Core settings first: a language change reloads the window, so the
      // client save (which may carry it) goes last and only after the core
      // save succeeded.
      if (generalDirty) {
        config.value = await updateGeneral($state.snapshot(general!));
        baseGeneral = JSON.stringify(general);
      }
      if (clientDirty) {
        await client.save($state.snapshot(draft!));
        baseClient = JSON.stringify(draft);
      }
      // The shell reads the language once at start, so a change needs a
      // reload for every page to render in it.
      if (languageChanged) { location.reload(); return; }
      toast.ok(t("common.saved"));
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      saving = false;
    }
  }

  async function setAutostart(on: boolean) {
    try {
      if (on) await enable(); else await disable();
    } catch (e) {
      toast.error(errorMessage(e));
    }
    // Show the real state whether or not the change went through.
    try { autostart = await isEnabled(); } catch { autostart = !on; }
  }

  async function askAccessibility() {
    try { accessibility = await requestAccessibility(); } catch (e) { toast.error(errorMessage(e)); }
  }

  async function openPane(pane: "accessibility" | "microphone") {
    try { await openPrivacyPane(pane); } catch (e) { toast.error(errorMessage(e)); }
  }

  async function setPlacing(on: boolean) {
    try { await widgetPlacing(on); placing = on; } catch (e) { toast.error(errorMessage(e)); }
  }

  async function resetWidget() {
    try { await widgetResetPosition(); toast.ok(t("general.widget_reset_done")); } catch (e) { toast.error(errorMessage(e)); }
  }

  function addApp(id: string) {
    if (draft && !(id in draft.insertion.per_app)) draft.insertion.per_app[id] = "paste";
  }

  function setMethod(bundle: string, method: string) {
    draft!.insertion.per_app[bundle] = method as InsertMethod;
  }
</script>

<PageHead title={t("nav.general")} />

<Group title={t("general.permissions")} help={draft?.hotkey.shortcuts.some((s) => s.includes("Fn")) ? t("general.fn_key_note") : undefined}>
  <Row label={t("general.accessibility")} help={t("general.accessibility_help")}>
    {#snippet leading()}<Ring tone={accessibility === null ? "off" : accessibility ? "ok" : "live"} />{/snippet}
    {#if accessibility === false}
      <button type="button" class="btn" onclick={askAccessibility}>{t("general.request")}</button>
      <button type="button" class="btn quiet" onclick={() => openPane("accessibility")}>{t("general.open_system_settings")}</button>
    {:else if accessibility}
      <span class="value">{t("common.granted")}</span>
    {/if}
  </Row>
  <Row label={t("general.microphone_permission")}>
    {#snippet leading()}<span class="ring-space"></span>{/snippet}
    <button type="button" class="btn quiet" onclick={() => openPane("microphone")}>{t("general.open_system_settings")}</button>
  </Row>
</Group>

{#if draft}
  <Group title={t("general.hotkey")}>
    <div class="shortcuts">
      <div class="shortcuts-head">
        <span class="name">{t("general.shortcuts")}</span>
        <button type="button" class="btn quiet" disabled={isDefaultShortcuts(draft.hotkey.shortcuts)} onclick={() => (draft!.hotkey.shortcuts = defaultShortcuts())}>{t("general.shortcut_reset")}</button>
      </div>
      <small class="sub">{t("general.shortcuts_help", { max: String(MAX_SHORTCUTS) })}</small>
      <ShortcutField bind:value={draft.hotkey.shortcuts} />
    </div>
    <Row label={t("general.hold_threshold")}>
      <NumberField bind:value={draft.hotkey.hold_threshold_ms} min={100} max={1000} unit={t("common.ms")} label={t("general.hold_threshold")} />
    </Row>
    <Row label={t("general.double_tap_window")}>
      <NumberField bind:value={draft.hotkey.double_tap_window_ms} min={150} max={1000} unit={t("common.ms")} label={t("general.double_tap_window")} />
    </Row>
    <Row label={t("general.max_utterance")}>
      <NumberField bind:value={draft.max_utterance_secs} min={10} max={1800} unit={t("common.seconds")} label={t("general.max_utterance")} />
    </Row>
    <Row label={t("general.auto_stop")} help={t("general.auto_stop_help")}>
      <Switch bind:checked={draft.auto_stop.enabled} label={t("general.auto_stop")} />
    </Row>
    {#if draft.auto_stop.enabled}
      <Row label={t("general.auto_stop_secs")}>
        <NumberField bind:value={draft.auto_stop.silence_secs} min={1} max={30} unit={t("common.seconds")} label={t("general.auto_stop_secs")} />
      </Row>
    {/if}
  </Group>

  <Group title={t("general.microphone")}>
    <Row label={t("general.input_device")}>
      <select class="wide" bind:value={draft.microphone} aria-label={t("general.input_device")}>
        <option value="builtin">{t("general.mic_builtin")}</option>
        <option value="default">{t("general.mic_default")}</option>
        {#each mics as m (m)}<option value={m}>{m}</option>{/each}
        {#if micAbsent}<option value={draft.microphone} disabled>{draft.microphone}</option>{/if}
      </select>
    </Row>
  </Group>

  <Group title={t("general.context")} help={t("general.context_help")}>
    <Row label={t("general.ctx_app_name")}><Switch bind:checked={draft.context.app_name} label={t("general.ctx_app_name")} /></Row>
    <Row label={t("general.ctx_window_title")}><Switch bind:checked={draft.context.window_title} label={t("general.ctx_window_title")} /></Row>
    <Row label={t("general.ctx_url")}><Switch bind:checked={draft.context.url} label={t("general.ctx_url")} /></Row>
    <Row label={t("general.ctx_selected_text")}><Switch bind:checked={draft.context.selected_text} label={t("general.ctx_selected_text")} /></Row>
    <Row label={t("general.ctx_clipboard")} help={t("general.ctx_clipboard_help")}><Switch bind:checked={draft.context.clipboard} label={t("general.ctx_clipboard")} /></Row>
  </Group>

  <Group title={t("general.insertion")} help={t("general.insertion_note")}>
    {#each Object.entries(draft.insertion.per_app) as [bundle, method] (bundle)}
      <Row>
        <span class="bundle" title={bundle}>{appName(bundle)}</span>
        <select class="method" value={method} onchange={(e) => setMethod(bundle, e.currentTarget.value)} aria-label={t("general.method")}>
          <option value="paste">{t("general.method_paste")}</option>
          <option value="type">{t("general.method_type")}</option>
          <option value="ax">{t("general.method_ax")}</option>
        </select>
        <button type="button" class="btn quiet icon-btn" onclick={() => { delete draft!.insertion.per_app[bundle]; }} aria-label={t("common.remove")} title={t("common.remove")}><Icon name="trash" /></button>
      </Row>
    {/each}
    <AddPerApp onadd={addApp} bind:value={newApp} />
  </Group>

  <Group title={t("general.app_group")}>
    <Row label={t("general.language")}>
      <select class="wide" bind:value={draft.ui_language} aria-label={t("general.language")}>
        <option value="system">{t("general.lang_system")}</option>
        <option value="zh-tw">繁體中文</option>
        <option value="en">English</option>
      </select>
    </Row>
    <Row label={t("general.launch_at_login")} help={t("general.applies_now")}>
      <Switch bind:checked={autostart} label={t("general.launch_at_login")} onchange={setAutostart} />
    </Row>
  </Group>
{/if}

<Group title={t("general.widget")}>
  <Row label={t("general.widget_position")} help={t("general.widget_position_help")}>
    <button type="button" class="btn" aria-pressed={placing} onclick={() => setPlacing(!placing)}>{t(placing ? "general.widget_done" : "general.widget_show")}</button>
    <button type="button" class="btn quiet" onclick={resetWidget}>{t("general.widget_reset")}</button>
  </Row>
  {#if draft}
    <Row label={t("general.widget_size")}>
      <select bind:value={draft.widget_size} aria-label={t("general.widget_size")}>
        <option value="small">{t("general.widget_size_small")}</option>
        <option value="medium">{t("general.widget_size_medium")}</option>
        <option value="large">{t("general.widget_size_large")}</option>
      </select>
    </Row>
    <Row label={t("general.show_transcript")} help={t("general.show_transcript_help")}>
      <Switch bind:checked={draft.show_transcript} label={t("general.show_transcript")} />
    </Row>
  {/if}
</Group>

{#if general}
  <Group title={t("general.history")}>
    <Row label={t("general.history_retention")} help={t("general.retention_zero_help")}>
      <NumberField bind:value={general.history_retention_days} min={0} unit={t("common.days")} label={t("general.history_retention")} />
    </Row>
    <Row label={t("general.keep_recordings")} help={t("general.keep_recordings_help")}>
      <Switch bind:checked={general.keep_recordings} label={t("general.keep_recordings")} />
    </Row>
    {#if folder}
      <div class="folder">
        <div class="folder-head">
          <span class="name">{t("general.recordings_dir")}</span>
          <span class="folder-actions">
            <button type="button" class="btn quiet" onclick={openFolder}>{t("general.recordings_dir_open")}</button>
            <button type="button" class="btn quiet" disabled={moving || folder.is_default} onclick={() => moveRecordings(null)}>{t("general.recordings_dir_reset")}</button>
            <button type="button" class="btn" disabled={moving} onclick={chooseFolder}>{t("general.recordings_dir_choose")}</button>
          </span>
        </div>
        <small class="sub">{folder.is_default ? t("general.recordings_dir_default", { path: folder.path }) : folder.path}</small>
      </div>
    {/if}
    <Row label={t("general.recording_retention")} help={t("general.retention_zero_help")}>
      <NumberField bind:value={general.recording_retention_days} min={0} unit={t("common.days")} label={t("general.recording_retention")} />
    </Row>
    <Row label={t("general.recordings_usage")} help={storage ? t("general.recordings_usage_value", { n: storage.recordings.toLocaleString(), size: formatBytes(storage.recording_bytes) }) : undefined}>
      <button type="button" class="btn danger" disabled={!storage?.recordings} onclick={() => (confirmRecordings = true)}>{t("history.clear_recordings")}</button>
    </Row>
    <Row label={t("general.typing_speed")} help={t("general.typing_speed_help")}>
      <NumberField bind:value={general.typing_chars_per_min} min={1} max={1000} unit={t("common.chars")} label={t("general.typing_speed")} />
    </Row>
    <Row label={t("general.stats")} help={t("general.stats_help")}>
      <button type="button" class="btn danger" onclick={() => (confirmStats = true)}>{t("general.stats_reset")}</button>
    </Row>
  </Group>

  <Group title={t("general.processing")}>
    <Row label={t("general.asr_timeout")}>
      <NumberField bind:value={general.asr_timeout_secs} min={1} unit={t("common.seconds")} label={t("general.asr_timeout")} />
    </Row>
    <Row label={t("general.llm_timeout")}>
      <NumberField bind:value={general.llm_timeout_secs} min={1} unit={t("common.seconds")} label={t("general.llm_timeout")} />
    </Row>
    <Row label={t("general.retry_count")} help={t("general.retry_count_help")}>
      <NumberField bind:value={general.retry_count} min={0} max={5} unit={t("common.times")} label={t("general.retry_count")} />
    </Row>
    <Row label={t("general.llm_idle_unload")} help={t("general.llm_idle_unload_help")}>
      <NumberField bind:value={general.llm_idle_unload_secs} min={0} unit={t("common.seconds")} label={t("general.llm_idle_unload")} />
    </Row>
  </Group>
{/if}

<SaveBar {dirty} {saving} message={problem} onsave={save} onrevert={reset} />

<Modal bind:open={confirmRecordings} title={t("history.clear_recordings_title")}>
  <p>{t("history.clear_recordings_body")}</p>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (confirmRecordings = false)}>{t("common.cancel")}</button>
    <button type="button" class="btn danger" onclick={clearAllRecordings}>{t("history.clear_recordings")}</button>
  {/snippet}
</Modal>

<Modal bind:open={confirmStats} title={t("general.stats_reset_title")}>
  <p>{t("general.stats_reset_body")}</p>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (confirmStats = false)}>{t("common.cancel")}</button>
    <button type="button" class="btn danger" onclick={resetStatistics}>{t("general.stats_reset")}</button>
  {/snippet}
</Modal>

<style>
  .value { color: var(--ink-2); }
  /* The shortcut row: its label on top, the shortcuts below over the full width. */
  .shortcuts { padding: var(--row-y) var(--row-x) 6px; border-bottom: 1px solid var(--line); }
  .shortcuts-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .shortcuts-head .btn { margin: -8px -8px -8px 0; }
  .shortcuts .sub { display: block; margin-top: 2px; }
  /* The folder row: its label and buttons on top, the path below over the full width. */
  .folder { padding: var(--row-y) var(--row-x); border-top: 1px solid var(--line); border-bottom: 1px solid var(--line); margin-bottom: -1px; }
  .folder-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .folder-actions { display: flex; align-items: center; gap: 8px; flex: none; }
  .folder .sub { display: block; margin-top: 6px; color: var(--ink-2); font-size: var(--text-sm); overflow-wrap: anywhere; }
  .ring-space { width: 14px; flex: none; }
  /* The selects of the value column share one width. */
  .wide { width: 220px; }
  .method { width: 170px; }
  .bundle { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .icon-btn { width: var(--control); padding: 0; margin-right: -8px; color: var(--ink-2); }
  .icon-btn:hover { color: var(--ink); }
</style>
