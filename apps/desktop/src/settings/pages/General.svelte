<script lang="ts">
  import { onMount } from "svelte";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
  import { errorMessage, listMicrophones, openPrivacyPane, permissions, requestAccessibility, updateGeneral, type ClientSettings, type General } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import { client, config } from "../../lib/store.svelte";
  import AddPerApp from "./AddPerApp.svelte";

  let mics = $state<string[]>([]);
  let accessibility = $state<boolean | null>(null);
  let autostart = $state(false);
  let status = $state("");
  let draft = $state<ClientSettings | null>(null);
  let general = $state<General | null>(null);
  let loadedUiLanguage = "";

  onMount(async () => {
    mics = await listMicrophones();
    accessibility = (await permissions()).accessibility;
    autostart = await isEnabled();
    draft = structuredClone($state.snapshot(client.value!));
    loadedUiLanguage = draft.ui_language;
    // `config.value` is null when the engine failed to open; the
    // client-settings section above still works since `client.json`
    // loads independently, but there is no core config to read here.
    if (config.value) general = structuredClone($state.snapshot(config.value.general));
  });

  async function saveClient() {
    if (!draft) return;
    try {
      await client.save(draft);
      status = t("common.saved");
      // The shell only reads `ui_language` at mount, so a change needs a
      // reload for every page (including this one) to re-render in it.
      if (draft.ui_language !== loadedUiLanguage) location.reload();
    } catch (e) { status = errorMessage(e); }
  }
  async function saveGeneral() {
    if (!general) return;
    try { await updateGeneral(general); status = t("common.saved"); } catch (e) { status = errorMessage(e); }
  }
  async function toggleAutostart() {
    if (autostart) await disable(); else await enable();
    autostart = await isEnabled();
  }
  async function askAccessibility() {
    accessibility = await requestAccessibility();
  }
</script>

<h2>{t("nav.general")}</h2>

<section>
  <h3>{t("general.permissions")}</h3>
  <div class="row">
    <span>{t("general.accessibility")}: {accessibility === null ? "..." : accessibility ? t("common.granted") : t("common.missing")}</span>
    {#if accessibility === false}
      <button onclick={askAccessibility}>{t("general.request")}</button>
      <button onclick={() => openPrivacyPane("accessibility")}>{t("general.open_system_settings")}</button>
    {/if}
  </div>
  <div class="row">
    <span>{t("general.microphone_permission")}</span>
    <button onclick={() => openPrivacyPane("microphone")}>{t("general.open_system_settings")}</button>
  </div>
  <p class="muted">{t("general.fn_key_note")}</p>
</section>

{#if draft}
<section>
  <h3>{t("general.hotkey")}</h3>
  <label>{t("general.hold_threshold")} <input type="number" min="100" max="1000" bind:value={draft.hotkey.hold_threshold_ms} /> ms</label>
  <label>{t("general.double_tap_window")} <input type="number" min="150" max="1000" bind:value={draft.hotkey.double_tap_window_ms} /> ms</label>
  <label>{t("general.max_utterance")} <input type="number" min="10" max="1800" bind:value={draft.max_utterance_secs} /> s</label>
</section>

<section>
  <h3>{t("general.microphone")}</h3>
  <select bind:value={draft.microphone}>
    <option value="builtin">{t("general.mic_builtin")}</option>
    <option value="default">{t("general.mic_default")}</option>
    {#each mics as m}<option value={m}>{m}</option>{/each}
  </select>
</section>

<section>
  <h3>{t("general.context")}</h3>
  <label><input type="checkbox" bind:checked={draft.context.app_name} /> {t("general.ctx_app_name")}</label>
  <label><input type="checkbox" bind:checked={draft.context.window_title} /> {t("general.ctx_window_title")}</label>
  <label><input type="checkbox" bind:checked={draft.context.url} /> {t("general.ctx_url")}</label>
  <label><input type="checkbox" bind:checked={draft.context.selected_text} /> {t("general.ctx_selected_text")}</label>
  <label><input type="checkbox" bind:checked={draft.context.clipboard} /> {t("general.ctx_clipboard")}</label>
</section>

<section>
  <h3>{t("general.insertion")}</h3>
  <p class="muted">{t("general.insertion_note")}</p>
  {#each Object.entries(draft.insertion.per_app) as [bundle, method]}
    <div class="row">
      <code>{bundle}</code>
      <select value={method} onchange={(e) => { draft!.insertion.per_app[bundle] = (e.currentTarget as HTMLSelectElement).value as "paste" | "type" | "ax"; }}>
        <option value="paste">{t("general.method_paste")}</option>
        <option value="type">{t("general.method_type")}</option>
        <option value="ax">{t("general.method_ax")}</option>
      </select>
      <button onclick={() => { delete draft!.insertion.per_app[bundle]; }}>{t("common.remove")}</button>
    </div>
  {/each}
  <AddPerApp onadd={(id) => { draft!.insertion.per_app[id] = "paste"; }} />
</section>

<section>
  <h3>{t("general.language")}</h3>
  <select bind:value={draft.ui_language}>
    <option value="system">{t("general.lang_system")}</option>
    <option value="zh-tw">繁體中文</option>
    <option value="en">English</option>
  </select>
</section>

<section>
  <label><input type="checkbox" checked={autostart} onchange={toggleAutostart} /> {t("tray.launch_at_login")}</label>
</section>

<button class="primary" onclick={saveClient}>{t("common.save")}</button>
{/if}

{#if general}
<section>
  <h3>{t("general.history")}</h3>
  <label>{t("general.history_retention")} <input type="number" min="1" bind:value={general.history_retention_days} /> {t("common.days")}</label>
  <label><input type="checkbox" bind:checked={general.keep_recordings} /> {t("general.keep_recordings")}</label>
  <label>{t("general.recording_retention")} <input type="number" min="1" bind:value={general.recording_retention_days} /> {t("common.days")}</label>
  <button class="primary" onclick={saveGeneral}>{t("common.save")}</button>
</section>
{/if}

<p class="status">{status}</p>
