<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { errorMessage, formatBytes, historyStorage, openRecordingsDir, recordingsDir, resetStats, setRecordingsDir, type RecordingsDir, listDisplays, listMicrophones, onWidgetPlacing, onWidgetPreviewScene, widgetIsPlacing, widgetPreview, openPrivacyPane, permissions, requestAccessibility, requestMicrophone, updateGeneral, widgetPlacing, widgetResetPosition, type ClientSettings, type DisplayName, type General, type HistoryStorage, type InsertMethod, type Permissions } from "../../lib/api";
  import { appName, ensureAppNames } from "../../lib/apps.svelte";
  import { t } from "../../lib/i18n";
  import { client, config, defaults } from "../../lib/store.svelte";
  import { resetOf, together } from "../../lib/reset";
  import { invalidFields } from "../../lib/numbers";
  import { guard } from "../../lib/guard.svelte";
  import { toast } from "../../lib/ui/toast.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Fold from "../../lib/ui/Fold.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Ring from "../../lib/ui/Ring.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import Segmented from "../../lib/ui/Segmented.svelte";
  import Switch from "../../lib/ui/Switch.svelte";
  import NumberField from "../../lib/ui/NumberField.svelte";
  import ShortcutField from "../../lib/ui/ShortcutField.svelte";
  import { shortcutLabel } from "../../lib/shortcut";
  import ResetButton from "../../lib/ui/ResetButton.svelte";
  import Choices from "../../lib/ui/Choices.svelte";
  import SaveBar from "../../lib/ui/SaveBar.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import PageLink from "../../lib/ui/PageLink.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import AddPerApp from "./AddPerApp.svelte";
  import { DEMO, SCENES, lookOf, previewRequest, sceneFor, type Look, type Scene } from "../../lib/widgetPreview";

  import type { SettingsPage } from "../../lib/route";

  /** Which part of the settings this page shows. The parts share one draft and one Save. */
  /** A settings page, or "processing": the engine settings shown at the foot of the Engines page. */
  let { section }: { section: SettingsPage | "processing" } = $props();

  let mics = $state<string[]>([]);
  let perms = $state<Permissions | null>(null);
  const accessibility = $derived(perms ? perms.accessibility : null);
  const microphone = $derived(perms ? perms.microphone : null);
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
  // What the previewed widget shows, and whether a slider of the faint widget is being dragged.
  let scene = $state<Scene>("place");
  let sliding = $state(false);
  // What the app was last asked to show; null while nothing is previewed.
  let shown: { scene: Scene; look: Look } | null = null;
  let demo: ReturnType<typeof setTimeout> | undefined;

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

  // A choice made in a menu takes its place in the draft; other unsaved edits stay.
  $effect(() => {
    const idle = client.value?.widget_idle;
    untrack(() => {
      if (!idle || !draft || !baseClient || JSON.parse(baseClient).widget_idle === idle) return;
      draft.widget_idle = idle;
      baseClient = JSON.stringify({ ...JSON.parse(baseClient), widget_idle: idle });
    });
  });

  // While the widget is previewed it shows the draft: every change of it, and the chosen state.
  $effect(() => {
    if (!placing || !draft) { shown = null; return; }
    const look = lookOf(draft);
    const now = scene, faint = sliding;
    const sample = { raw: t("general.preview_raw"), refined: t("general.preview_refined"), detail: t("general.preview_detail") };
    untrack(() => {
      // A changed setting is shown in the state it applies to.
      const fitting = shown && !demo ? sceneFor(shown.look, look) : null;
      if (fitting && fitting !== now) {
        shown = { scene: shown!.scene, look };
        scene = fitting;
        return;
      }
      const play = !shown || shown.scene !== now || shown.look.show_transcript !== look.show_transcript;
      shown = { scene: now, look };
      widgetPreview(previewRequest(now, look, sample, play, faint)).catch(() => {});
    });
  });

  function stopDemo() {
    clearTimeout(demo);
    demo = undefined;
  }
  /** Plays one dictation through: listening, processing, done, and back to rest. */
  function playDemo() {
    stopDemo();
    let i = 0;
    const next = () => {
      const [to, ms] = DEMO[i++];
      scene = to;
      demo = ms ? setTimeout(next, ms) : undefined;
    };
    next();
  }

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
    try { perms = await permissions(); } catch { perms = null; }
  }

  // What the history takes on disk, and the confirmation of the statistics reset.
  let storage = $state<HistoryStorage | null>(null);
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
  async function resetStatistics() {
    confirmStats = false;
    try { await resetStats(); toast.ok(t("general.stats_reset_done")); } catch (e) { toast.error(errorMessage(e)); }
  }

  const scenes = $derived(SCENES.map((value) => ({ value, label: t(`general.preview_scene_${value}`) })));
  const idles = $derived((["hidden", "ring", "label"] as const).map((value) => ({ value, label: t(`general.widget_idle_${value}`) })));
  const sizes = $derived((["x_small", "small", "medium", "large", "x_large"] as const).map((value) => ({ value, label: t(`general.widget_size_${value}`) })));

  const languages = $derived([{ value: "system" as const, label: t("general.lang_system") }, { value: "zh-tw" as const, label: "正體中文" }, { value: "en" as const, label: "English" }]);
  const methods = $derived((["paste", "type", "ax", "copy"] as const).map((value) => ({ value, label: t(`general.method_${value}`), help: t(`general.method_${value}_help`) })));

  // The way back to the default, for each setting that has one and differs from it.
  const unit = (name: string) => (v: unknown) => `${v} ${t(name)}`;
  const onOff = (v: unknown) => t(v ? "common.on" : "common.off");
  const named = (prefix: string) => (v: unknown) => t(prefix + v);
  const ofClient = (path: string, show: (v: unknown) => string) => resetOf(draft, defaults.value?.client ?? null, path, (v) => t("common.reset_to", { value: show(v) }));
  const ofGeneral = (path: string, show: (v: unknown) => string) => resetOf(general, defaults.value?.general ?? null, path, (v) => t("common.reset_to", { value: show(v) }));
  const resets = $derived({
    size: ofClient("widget_size", named("general.widget_size_")),
    display: ofClient("widget_display", () => t("general.widget_display_pointer")),
    idle: ofClient("widget_idle", named("general.widget_idle_")),
    fade: ofClient("widget_fade.enabled", onOff),
    fadeAfter: ofClient("widget_fade.after_secs", unit("common.seconds")),
    fadeTransparency: ofClient("widget_fade.transparency", (v) => `${v}%`),
    transcript: ofClient("show_transcript", onOff),
    shortcuts: ofClient("hotkey.shortcuts", (v) => (v as string[][]).map(shortcutLabel).join(", ")),
    hold: ofClient("hotkey.hold_threshold_ms", unit("common.ms")),
    doubleTap: ofClient("hotkey.double_tap_window_ms", unit("common.ms")),
    escape: ofClient("hotkey.escape_cancels_processing", onOff),
    mic: ofClient("microphone", () => t("general.mic_builtin")),
    maxUtterance: ofClient("max_utterance_secs", unit("common.seconds")),
    autoStop: ofClient("auto_stop.enabled", onOff),
    autoStopSecs: ofClient("auto_stop.silence_secs", unit("common.seconds")),
    historyDays: ofGeneral("history_retention_days", unit("common.days")),
    stageDays: ofGeneral("stage_retention_days", unit("common.days")),
    keepRecordings: ofGeneral("keep_recordings", onOff),
    recordingDays: ofGeneral("recording_retention_days", unit("common.days")),
    typing: ofGeneral("typing_chars_per_min", unit("common.chars")),
    ctxApp: ofClient("context.app_name", onOff),
    ctxTitle: ofClient("context.window_title", onOff),
    ctxUrl: ofClient("context.url", onOff),
    ctxSelected: ofClient("context.selected_text", onOff),
    ctxClipboard: ofClient("context.clipboard", onOff),
    asrTimeout: ofGeneral("asr_timeout_secs", unit("common.seconds")),
    llmTimeout: ofGeneral("llm_timeout_secs", unit("common.seconds")),
    retry: ofGeneral("retry_count", unit("common.times")),
    idleUnload: ofGeneral("llm_idle_unload_secs", unit("common.seconds")),
    method: ofClient("insertion.default", named("general.method_")),
    keepClipboard: ofClient("insertion.keep_on_clipboard", onOff),
  });
  // A group offers the resets of the rows it shows.
  const fadeShown = $derived(draft?.widget_idle !== "hidden");
  const widgetResets = $derived(together([resets.size, resets.display, resets.idle, ...(fadeShown ? [resets.fade, ...(draft?.widget_fade.enabled ? [resets.fadeAfter, resets.fadeTransparency] : [])] : []), resets.transcript]));
  const lengthResets = $derived(together([resets.maxUtterance, resets.autoStop, ...(draft?.auto_stop.enabled ? [resets.autoStopSecs] : [])]));

  // The connected displays, for the choice of the one the widget stays on.
  let displays = $state<DisplayName[]>([]);

  onMount(() => {
    listDisplays().then((d) => { displays = d; }).catch(() => {});
    // The same switch is in the menus.
    const placed = onWidgetPlacing((on) => { stopDemo(); scene = "place"; placing = on; });
    // A real dictation or a drag took the place of the state that was shown.
    const scened = onWidgetPreviewScene((to) => { stopDemo(); scene = to; });
    widgetIsPlacing().then((on) => { placing = on; }).catch(() => {});
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
      placed.then((off) => off());
      scened.then((off) => off());
      stopDemo();
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
      { key: "general.stage_retention", value: general.stage_retention_days, min: 0, integer: true },
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
    try { await requestAccessibility(); await checkAccess(); } catch (e) { toast.error(errorMessage(e)); }
  }

  async function askMicrophone() {
    try { await requestMicrophone(); await checkAccess(); } catch (e) { toast.error(errorMessage(e)); }
  }

  async function openPane(pane: "accessibility" | "microphone") {
    try { await openPrivacyPane(pane); } catch (e) { toast.error(errorMessage(e)); }
  }

  async function setPlacing(on: boolean) {
    stopDemo();
    scene = "place";
    try { await widgetPlacing(on); placing = on; } catch (e) { toast.error(errorMessage(e)); }
  }

  async function resetWidget() {
    try { await widgetResetPosition(); toast.ok(t("general.widget_reset_done")); } catch (e) { toast.error(errorMessage(e)); }
  }

  function addApp(id: string) {
    if (draft && !(id in draft.insertion.per_app)) draft.insertion.per_app[id] = draft.insertion.default;
  }

  function setMethod(bundle: string, method: string) {
    draft!.insertion.per_app[bundle] = method as InsertMethod;
  }
</script>

{#if section !== "processing"}<PageHead title={t("nav." + section)} />{/if}

{#snippet notAllowed()}<Tag tone="live">{t("general.permission_missing")}</Tag>{/snippet}
{#snippet notAsked()}<Tag tone="warn">{t("general.permission_not_asked")}</Tag>{/snippet}
{#snippet restricted()}<Tag tone="live">{t("general.permission_restricted")}</Tag>{/snippet}

{#if section === "general"}
  {#if draft}
    <Group title={t("general.app_group")}>
      <Row label={t("general.language")}>
        <Segmented options={languages} bind:value={draft.ui_language} label={t("general.language")} />
      </Row>
      <Row label={t("general.launch_at_login")} help={t("general.applies_now")}>
        <Switch bind:checked={autostart} label={t("general.launch_at_login")} onchange={setAutostart} />
      </Row>
    </Group>
  {/if}

  <Group title={t("general.permissions")} section="permissions" help={draft?.hotkey.shortcuts.some((s) => s.includes("Fn")) ? t("general.fn_key_note") : undefined}>
    <Row
      label={t("general.accessibility")}
      tag={accessibility === false ? notAllowed : undefined}
      help={accessibility === false
        ? t("general.accessibility_missing") + (perms?.keyboard_watch_only ? t("general.accessibility_not_input_monitoring") : "")
        : t("general.accessibility_help")}
    >
      {#snippet leading()}<Ring tone={accessibility === null ? "off" : accessibility ? "ok" : "live"} />{/snippet}
      {#if accessibility === false}
        <button type="button" class="btn" onclick={askAccessibility}>{t("general.request")}</button>
        <button type="button" class="btn quiet" onclick={() => openPane("accessibility")}>{t("general.open_system_settings")}</button>
      {:else if accessibility}
        <span class="value">{t("common.granted")}</span>
      {/if}
    </Row>
    <Row
      label={t("general.microphone_permission")}
      tag={microphone === "denied" ? notAllowed : microphone === "not_asked" ? notAsked : microphone === "restricted" ? restricted : undefined}
      help={t(microphone === "denied" ? "general.microphone_denied" : microphone === "not_asked" ? "general.microphone_not_asked" : microphone === "restricted" ? "general.microphone_restricted" : "general.microphone_help")}
    >
      {#snippet leading()}<Ring tone={microphone === "granted" ? "ok" : microphone === "denied" || microphone === "restricted" ? "live" : "off"} />{/snippet}
      {#if microphone === "granted"}
        <span class="value">{t("common.granted")}</span>
      {:else if microphone === "not_asked"}
        <button type="button" class="btn" onclick={askMicrophone}>{t("general.request")}</button>
      {:else if microphone === "denied"}
        <button type="button" class="btn" onclick={() => openPane("microphone")}>{t("general.open_system_settings")}</button>
      {/if}
    </Row>
  </Group>

  <Group title={t("general.widget")} onreset={widgetResets}>
    <Row label={t("general.widget_position")} help={t("general.widget_position_help")}>
      <button type="button" class="btn" aria-pressed={placing} onclick={() => setPlacing(!placing)}>{t(placing ? "general.widget_done" : "general.widget_show")}</button>
      <button type="button" class="btn quiet" onclick={resetWidget}>{t("general.widget_reset")}</button>
    </Row>
    {#if placing && draft}
      <Row label={t("general.preview_scene")} help={scene === "idle" && draft.widget_idle === "hidden" ? t("general.preview_idle_hidden") : undefined}>
        <span class="scenes">
          <Segmented options={scenes} bind:value={() => scene, (to) => { stopDemo(); scene = to; }} label={t("general.preview_scene")} />
          <button type="button" class="btn" onclick={playDemo}>{t("general.preview_play")}</button>
        </span>
      </Row>
    {/if}
    {#if draft}
      <Row label={t("general.widget_size")} reset={resets.size}>
        <Segmented options={sizes} bind:value={draft.widget_size} label={t("general.widget_size")} />
      </Row>
      <Row label={t("general.widget_display")} help={t("general.widget_display_help")} reset={resets.display}>
        <select class="wide" value={draft.widget_display ?? ""} onchange={(e) => { draft!.widget_display = e.currentTarget.value || null; }} aria-label={t("general.widget_display")}>
          <option value="">{t("general.widget_display_pointer")}</option>
          {#each displays as d (d.key)}<option value={d.key}>{d.name}</option>{/each}
          {#if draft.widget_display && !displays.some((d) => d.key === draft!.widget_display)}
            <option value={draft.widget_display}>{t("general.widget_display_missing", { name: draft.widget_display })}</option>
          {/if}
        </select>
      </Row>
      <Row label={t("general.widget_idle")} help={t("general.widget_idle_help")} reset={resets.idle}>
        <Segmented options={idles} bind:value={draft.widget_idle} label={t("general.widget_idle")} />
      </Row>
      {#if draft.widget_idle !== "hidden"}<Fold>
        <Row label={t("general.widget_fade")} help={t(draft.widget_fade.enabled ? "general.widget_fade_help" : "general.widget_fade_off")} reset={resets.fade}>
          <Switch bind:checked={draft.widget_fade.enabled} label={t("general.widget_fade")} />
        </Row>
        {#if draft.widget_fade.enabled}<Fold>
          <Row label={t("general.widget_fade_after")} reset={resets.fadeAfter}>
            <span class="range">
              <input type="range" min="1" max="60" step="1" bind:value={draft.widget_fade.after_secs} oninput={() => { sliding = true; }} onchange={() => { sliding = false; }} aria-label={t("general.widget_fade_after")} />
              <output>{draft.widget_fade.after_secs} {t("common.seconds")}</output>
            </span>
          </Row>
          <Row label={t("general.widget_fade_transparency")} reset={resets.fadeTransparency}>
            <span class="range">
              <input type="range" min="10" max="90" step="5" bind:value={draft.widget_fade.transparency} oninput={() => { sliding = true; }} onchange={() => { sliding = false; }} aria-label={t("general.widget_fade_transparency")} />
              <output>{draft.widget_fade.transparency}%</output>
            </span>
          </Row>
        </Fold>{/if}
      </Fold>{/if}
      <Row label={t("general.show_transcript")} help={t(draft.show_transcript ? "general.show_transcript_help" : "general.show_transcript_off")} reset={resets.transcript}>
        <Switch bind:checked={draft.show_transcript} label={t("general.show_transcript")} />
      </Row>
    {/if}
  </Group>

  {#if general}
    {#snippet typingHelp()}{t("general.typing_speed_help_before")}<PageLink page="home" label={t("nav.home")} />{t("general.typing_speed_help_after")}{/snippet}
    <Group title={t("nav.home")} onreset={together([resets.typing])}>
      <Row label={t("general.typing_speed")} help={typingHelp} reset={resets.typing}>
        <NumberField bind:value={general.typing_chars_per_min} min={1} max={1000} unit={t("common.chars")} label={t("general.typing_speed")} />
      </Row>
    </Group>
  {/if}
{:else if section === "recording"}
  {#if draft}
    <Group title={t("general.hotkey")} onreset={together([resets.shortcuts, resets.hold, resets.doubleTap, resets.escape])}>
      <div class="shortcuts">
        <div class="shortcuts-head">
          <span class="name">{t("general.shortcuts")}</span>
          <ResetButton reset={resets.shortcuts} />
        </div>
        <small class="sub">{t("general.shortcuts_help")}</small>
        <ShortcutField bind:value={draft.hotkey.shortcuts} />
      </div>
      <Row label={t("general.hold_threshold")} reset={resets.hold}>
        <NumberField bind:value={draft.hotkey.hold_threshold_ms} min={100} max={1000} unit={t("common.ms")} label={t("general.hold_threshold")} />
      </Row>
      <Row label={t("general.double_tap_window")} reset={resets.doubleTap}>
        <NumberField bind:value={draft.hotkey.double_tap_window_ms} min={150} max={1000} unit={t("common.ms")} label={t("general.double_tap_window")} />
      </Row>
      <Row label={t("general.escape_processing")} help={t(draft.hotkey.escape_cancels_processing ? "general.escape_processing_on" : "general.escape_processing_off")} reset={resets.escape}>
        <Switch bind:checked={draft.hotkey.escape_cancels_processing} label={t("general.escape_processing")} />
      </Row>
    </Group>

    <Group title={t("general.microphone")} onreset={together([resets.mic])}>
      <Row label={t("general.input_device")} reset={resets.mic}>
        <select class="wide" bind:value={draft.microphone} aria-label={t("general.input_device")}>
          <option value="builtin">{t("general.mic_builtin")}</option>
          <option value="default">{t("general.mic_default")}</option>
          {#each mics as m (m)}<option value={m}>{m}</option>{/each}
          {#if micAbsent}<option value={draft.microphone} disabled>{draft.microphone}</option>{/if}
        </select>
      </Row>
    </Group>

    <Group title={t("general.length")} onreset={lengthResets}>
      <Row label={t("general.max_utterance")} reset={resets.maxUtterance}>
        <NumberField bind:value={draft.max_utterance_secs} min={10} max={1800} unit={t("common.seconds")} label={t("general.max_utterance")} />
      </Row>
      <Row label={t("general.auto_stop")} help={t(draft.auto_stop.enabled ? "general.auto_stop_on" : "general.auto_stop_off")} reset={resets.autoStop}>
        <Switch bind:checked={draft.auto_stop.enabled} label={t("general.auto_stop")} />
      </Row>
      {#if draft.auto_stop.enabled}<Fold>
        <Row label={t("general.auto_stop_secs")} reset={resets.autoStopSecs}>
          <NumberField bind:value={draft.auto_stop.silence_secs} min={1} max={30} unit={t("common.seconds")} label={t("general.auto_stop_secs")} />
        </Row>
      </Fold>{/if}
    </Group>
  {/if}

  {#if draft}
    <Group title={t("general.method")} onreset={together([resets.method, resets.keepClipboard])}>
      <div class="default-method">
        <div class="shortcuts-head">
          <span class="name">{t("general.method_default")}</span>
          <ResetButton reset={resets.method} />
        </div>
        <small class="sub">{t("general.method_default_help")}</small>
        <Choices options={methods} bind:value={draft.insertion.default} label={t("general.method_default")} />
      </div>
      <Row label={t("general.keep_on_clipboard")} help={t(draft.insertion.keep_on_clipboard ? "general.keep_on_clipboard_on" : "general.keep_on_clipboard_off")} reset={resets.keepClipboard}>
        <Switch bind:checked={draft.insertion.keep_on_clipboard} label={t("general.keep_on_clipboard")} />
      </Row>
    </Group>

    <Group title={t("general.insertion")} help={t("general.insertion_note")}>
      {#each Object.entries(draft.insertion.per_app) as [bundle, method] (bundle)}
        <Row>
          <span class="bundle" title={bundle}>{appName(bundle)}</span>
          <select class="method" value={method} onchange={(e) => setMethod(bundle, e.currentTarget.value)} aria-label={t("general.method")}>
            <option value="paste">{t("general.method_paste")}</option>
            <option value="type">{t("general.method_type")}</option>
            <option value="ax">{t("general.method_ax")}</option>
            <option value="copy">{t("general.method_copy")}</option>
          </select>
          <button type="button" class="btn quiet icon-btn" onclick={() => { delete draft!.insertion.per_app[bundle]; }} aria-label={t("common.remove")} title={t("common.remove")}><Icon name="trash" /></button>
        </Row>
      {/each}
      <AddPerApp onadd={addApp} bind:value={newApp} />
    </Group>
  {/if}
{:else if section === "data"}
  {#if general}
    {#snippet statsHelp()}{t("general.stats_help_before")}<PageLink page="home" label={t("nav.home")} />{t("general.stats_help_after")}{/snippet}
    {#snippet storageHelp()}{#if storage}{t("general.storage_rows", { n: storage.rows.toLocaleString(), size: formatBytes(storage.text_bytes) })}{t("home.list_sep")}{t("general.storage_stages", { size: formatBytes(storage.stage_bytes) })}{t("home.list_sep")}{t("general.storage_recordings", { n: storage.recordings.toLocaleString(), size: formatBytes(storage.recording_bytes) })}<br />{/if}{t("general.storage_clear_before")}<PageLink page="history" label={t("nav.history")} />{t("general.storage_clear_after")}{/snippet}
    <Group title={t("general.history")} onreset={together([resets.historyDays, resets.stageDays, resets.keepRecordings, resets.recordingDays])}>
      <Row label={t("general.history_retention")} help={t("general.retention_zero_help")} reset={resets.historyDays}>
        <NumberField bind:value={general.history_retention_days} min={0} unit={t("common.days")} label={t("general.history_retention")} />
      </Row>
      <Row label={t("general.stage_retention")} help={t("general.stage_retention_help")} reset={resets.stageDays}>
        <NumberField bind:value={general.stage_retention_days} min={0} unit={t("common.days")} label={t("general.stage_retention")} />
      </Row>
      <Row label={t("general.keep_recordings")} help={t(general.keep_recordings ? "general.keep_recordings_on" : "general.keep_recordings_off")} reset={resets.keepRecordings}>
        <Switch bind:checked={general.keep_recordings} label={t("general.keep_recordings")} />
      </Row>
      {#if general.keep_recordings}<Fold>
        <Row label={t("general.recording_retention")} help={t("general.retention_zero_help")} reset={resets.recordingDays}>
          <NumberField bind:value={general.recording_retention_days} min={0} unit={t("common.days")} label={t("general.recording_retention")} />
        </Row>
      </Fold>{/if}
      <!-- The folder matters only while recordings are kept; files already there follow the retention days. -->
      {#if folder && general.keep_recordings}<Fold>
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
      </Fold>{/if}
      <Row label={t("general.storage")} help={storageHelp} />
      <Row label={t("general.stats")} help={statsHelp}>
        <button type="button" class="btn danger" onclick={() => (confirmStats = true)}>{t("general.stats_reset")}</button>
      </Row>
    </Group>

  {/if}

  {#if draft}
    <Group title={t("general.context")} help={t("general.context_help")} onreset={together([resets.ctxApp, resets.ctxTitle, resets.ctxUrl, resets.ctxSelected, resets.ctxClipboard])}>
      <Row label={t("general.ctx_app_name")} reset={resets.ctxApp}><Switch bind:checked={draft.context.app_name} label={t("general.ctx_app_name")} /></Row>
      <Row label={t("general.ctx_window_title")} reset={resets.ctxTitle}><Switch bind:checked={draft.context.window_title} label={t("general.ctx_window_title")} /></Row>
      <Row label={t("general.ctx_url")} reset={resets.ctxUrl}><Switch bind:checked={draft.context.url} label={t("general.ctx_url")} /></Row>
      <Row label={t("general.ctx_selected_text")} reset={resets.ctxSelected}><Switch bind:checked={draft.context.selected_text} label={t("general.ctx_selected_text")} /></Row>
      <Row label={t("general.ctx_clipboard")} help={t("general.ctx_clipboard_help")} reset={resets.ctxClipboard}><Switch bind:checked={draft.context.clipboard} label={t("general.ctx_clipboard")} /></Row>
    </Group>
  {/if}
{:else if section === "processing"}
  {#if general}
    <Group title={t("general.processing")} onreset={together([resets.asrTimeout, resets.llmTimeout, resets.retry, resets.idleUnload])}>
      <Row label={t("general.asr_timeout")} reset={resets.asrTimeout}>
        <NumberField bind:value={general.asr_timeout_secs} min={1} unit={t("common.seconds")} label={t("general.asr_timeout")} />
      </Row>
      <Row label={t("general.llm_timeout")} reset={resets.llmTimeout}>
        <NumberField bind:value={general.llm_timeout_secs} min={1} unit={t("common.seconds")} label={t("general.llm_timeout")} />
      </Row>
      <Row label={t("general.retry_count")} help={t("general.retry_count_help")} reset={resets.retry}>
        <NumberField bind:value={general.retry_count} min={0} max={5} unit={t("common.times")} label={t("general.retry_count")} />
      </Row>
      <Row label={t("general.llm_idle_unload")} help={t("general.llm_idle_unload_help")} reset={resets.idleUnload}>
        <NumberField bind:value={general.llm_idle_unload_secs} min={0} unit={t("common.seconds")} label={t("general.llm_idle_unload")} />
      </Row>
    </Group>
  {/if}

{/if}

<SaveBar {dirty} {saving} message={problem} onsave={save} onrevert={reset} />

<Modal bind:open={confirmStats} title={t("general.stats_reset_title")}>
  <p>{t("general.stats_reset_body")}</p>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (confirmStats = false)}>{t("common.cancel")}</button>
    <button type="button" class="btn danger" onclick={resetStatistics}>{t("general.stats_reset")}</button>
  {/snippet}
</Modal>

<style>
  .value { color: var(--ink-2); }
  .range { display: flex; align-items: center; gap: 12px; flex: none; }
  .scenes { display: flex; flex-wrap: wrap; justify-content: flex-end; align-items: center; gap: 8px; }
  .range input { width: 200px; accent-color: var(--accent); }
  .range output { min-width: 52px; text-align: right; color: var(--ink-2); font-variant-numeric: tabular-nums; }
  /* The shortcut row: its label on top, the shortcuts below over the full width. */
  .shortcuts { padding: var(--row-y) var(--row-x) 6px; border-bottom: 1px solid var(--line); }
  .shortcuts-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .shortcuts-head :global(.reset) { margin: -4px 0; }
  /* The default method: its label on top, the methods below over the full width. */
  .default-method { padding: var(--row-y) var(--row-x) 10px; }
  .default-method .sub { display: block; margin: 2px 0 8px; }
  .default-method :global(.choices) { margin: 0 -12px; }
  .shortcuts .sub { display: block; margin-top: 2px; }
  /* The folder row: its label and buttons on top, the path below over the full width. */
  .folder { padding: var(--row-y) var(--row-x); border-top: 1px solid var(--line); border-bottom: 1px solid var(--line); margin-bottom: -1px; }
  .folder-head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 4px 12px; }
  .folder-actions { display: flex; align-items: center; gap: 8px; flex: none; }
  .folder .sub { display: block; margin-top: 6px; color: var(--ink-2); font-size: var(--text-sm); overflow-wrap: anywhere; }
  /* The selects of the value column share one width. */
  .wide { width: 240px; }
  .method { width: 190px; }
  .bundle { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .icon-btn { width: var(--control); padding: 0; margin-right: -8px; color: var(--ink-2); }
  .icon-btn:hover { color: var(--ink); }
</style>
