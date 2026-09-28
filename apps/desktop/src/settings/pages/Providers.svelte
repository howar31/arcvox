<script lang="ts">
  import { deleteProvider, deleteSecret, errorMessage, hasSecret, probeProvider, setSecret, upsertProvider, type ProviderConfig } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import { config } from "../../lib/store.svelte";

  let editing = $state<ProviderConfig | null>(null);
  let apiKey = $state("");
  let keyPresent = $state(false);
  let status = $state("");
  let probe = $state("");

  function blank(): ProviderConfig {
    return { id: "", label: "", kind: "http", capabilities: { transcribe: false, chat: true, streaming: false, language_hint: false, hotwords: false },
      base_url: "http://localhost:11434/v1", model: "", api_key_ref: null, extra_headers: {}, model_path: null, native_id: null };
  }
  async function edit(p: ProviderConfig) {
    editing = structuredClone($state.snapshot(p));
    apiKey = "";
    keyPresent = p.api_key_ref ? await hasSecret(p.api_key_ref) : false;
    probe = "";
  }
  async function save() {
    if (!editing) return;
    const p = editing;
    if (!p.id) p.id = p.label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || `provider-${Date.now()}`;
    p.model = p.model?.trim() || null;
    p.base_url = p.base_url?.trim() || null;
    try {
      if (apiKey.trim()) { p.api_key_ref = p.id; await setSecret(p.id, apiKey); }
      else if (!keyPresent) { p.api_key_ref = null; }
      await upsertProvider(p);
      status = t("common.saved");
      editing = null;
    } catch (e) { status = errorMessage(e); }
  }
  async function remove(p: ProviderConfig) {
    try { await deleteProvider(p.id); if (p.api_key_ref) await deleteSecret(p.api_key_ref); status = t("common.saved"); }
    catch (e) { status = errorMessage(e); }
  }
  async function runProbe() {
    if (!editing) return;
    probe = "...";
    const r = await probeProvider(editing);
    probe = r.ok ? t("providers.probe_ok") : `${t("providers.probe_failed")}: ${r.failure?.kind ?? ""} ${r.failure?.message ?? ""}`;
  }
</script>

<h2>{t("nav.providers")}</h2>
<table>
  <thead><tr><th>{t("providers.label")}</th><th>{t("providers.base_url")}</th><th>{t("providers.model")}</th><th></th></tr></thead>
  <tbody>
    {#each config.value?.providers ?? [] as p}
      <tr><td>{p.label}</td><td>{p.base_url ?? ""}</td><td>{p.model ?? ""}</td>
        <td><button onclick={() => edit(p)}>{t("common.edit")}</button> <button onclick={() => remove(p)}>{t("common.remove")}</button></td></tr>
    {/each}
  </tbody>
</table>
<button onclick={() => edit(blank())}>{t("providers.add")}</button>

{#if editing}
<section class="form">
  <label>{t("providers.label")} <input bind:value={editing.label} /></label>
  <label>{t("providers.base_url")} <input bind:value={editing.base_url} placeholder="http://localhost:11434/v1" /></label>
  <label>{t("providers.model")} <input bind:value={editing.model} /></label>
  <label>{t("providers.api_key")} <input type="password" bind:value={apiKey} placeholder={keyPresent ? t("providers.key_present") : ""} /></label>
  <fieldset>
    <legend>{t("providers.capabilities")}</legend>
    <label><input type="checkbox" bind:checked={editing.capabilities.transcribe} /> {t("providers.cap_transcribe")}</label>
    <label><input type="checkbox" bind:checked={editing.capabilities.chat} /> {t("providers.cap_chat")}</label>
    <label><input type="checkbox" bind:checked={editing.capabilities.language_hint} /> {t("providers.cap_language_hint")}</label>
    <label><input type="checkbox" bind:checked={editing.capabilities.hotwords} /> {t("providers.cap_hotwords")}</label>
  </fieldset>
  <div class="row">
    <button onclick={runProbe}>{t("providers.probe")}</button><span>{probe}</span>
  </div>
  <div class="row">
    <button class="primary" onclick={save}>{t("common.save")}</button>
    <button onclick={() => (editing = null)}>{t("common.cancel")}</button>
  </div>
</section>
{/if}
<p class="status">{status}</p>
