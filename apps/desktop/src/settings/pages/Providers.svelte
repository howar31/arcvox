<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import {
    deleteProvider, deleteSecret, errorMessage, hasSecret, modelsInstalled, nativeInstallLanguage, nativeStatus, onNativeInstallProgress,
    probeProvider, setSecret, upsertProvider, type InstalledModel, type NativeStatusView, type ProviderConfig, type ProviderKind, type Runtime,
  } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import { config } from "../../lib/store.svelte";
  import { baseName, blankFor, capsForNative, capsForRuntime, describeProbe, NATIVE_DICTATION, NATIVE_FM } from "../../lib/providerForm";

  let editing = $state<ProviderConfig | null>(null);
  let apiKey = $state("");
  let keyPresent = $state(false);
  let status = $state("");
  let probe = $state("");
  // Per-row probe result, keyed by provider id, shown next to that row.
  let rowProbes = $state<Record<string, string>>({});
  let installed = $state<InstalledModel[]>([]);
  let native = $state<NativeStatusView | null>(null);
  let installing = $state(false);
  let installFraction = $state(0);
  let unlisten: (() => void) | undefined;

  const defaultHint = $derived(config.value?.profiles.find((p) => p.is_default)?.language_hint ?? null);
  const candidates = $derived(installed.filter((m) => m.runtime === editing?.runtime));
  // The select value encodes either a catalogue id or `custom:<path>`.
  const selectedModel = $derived(editing?.model_path ? `custom:${editing.model_path}` : (editing?.model_id ?? ""));

  onMount(async () => {
    unlisten = await onNativeInstallProgress((f) => (installFraction = f));
  });
  onDestroy(() => unlisten?.());

  async function refreshInstalled() {
    try { installed = await modelsInstalled(); } catch (e) { status = errorMessage(e); }
  }
  // Only the most recent request may update the status line.
  let nativeRequest = 0;
  async function refreshNative() {
    const req = ++nativeRequest;
    if (editing?.kind !== "native" || !editing.native_id) { native = null; return; }
    let next: NativeStatusView;
    try { next = await nativeStatus(editing.native_id, defaultHint); } catch (e) { next = { status: "failed", reason: errorMessage(e) }; }
    if (req === nativeRequest) native = next;
  }
  const STATUSES = ["ready", "needs_download", "locale_unsupported", "os_too_old", "not_eligible", "intelligence_off", "model_not_ready", "failed"];
  const installError = (e: unknown) => {
    const m = errorMessage(e);
    return STATUSES.includes(m) ? t("providers.native." + m) : m;
  };
  const missingModel = $derived(editing?.kind === "embedded" && !editing.model_id && !editing.model_path);
  function open(p: ProviderConfig) {
    editing = structuredClone($state.snapshot(p));
    probe = "";
    native = null;
    if (editing.kind === "embedded") refreshInstalled();
    if (editing.kind === "native") refreshNative();
  }
  async function edit(p: ProviderConfig) {
    open(p);
    apiKey = "";
    keyPresent = p.api_key_ref ? await hasSecret(p.api_key_ref) : false;
  }
  function setKind(kind: ProviderKind) {
    if (!editing) return;
    // Keep what the user already typed; the rest comes from the new kind's blank.
    const fresh = blankFor(kind);
    fresh.id = editing.id;
    fresh.label = editing.label;
    open(fresh);
  }
  function setRuntime(r: Runtime) {
    if (!editing) return;
    editing.runtime = r;
    editing.capabilities = capsForRuntime(r);
    editing.model_id = null;
    editing.model_path = null;
  }
  function setNativeId(id: string) {
    if (!editing) return;
    editing.native_id = id;
    editing.capabilities = capsForNative(id);
    refreshNative();
  }
  function setModel(value: string) {
    if (!editing) return;
    if (value.startsWith("custom:")) { editing.model_path = value.slice("custom:".length); editing.model_id = null; }
    else { editing.model_id = value || null; editing.model_path = null; }
  }
  async function installLanguage() {
    installing = true;
    installFraction = 0;
    try { await nativeInstallLanguage(defaultHint); }
    catch (e) { status = installError(e); }
    installing = false;
    await refreshNative();
  }
  async function save() {
    if (!editing) return;
    const p = editing;
    if (!p.id) p.id = p.label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || `provider-${Date.now()}`;
    p.model = p.model?.trim() || null;
    p.base_url = p.base_url?.trim() || null;
    try {
      if (p.kind === "http") {
        if (apiKey.trim()) { p.api_key_ref = p.id; await setSecret(p.id, apiKey); }
        else if (!keyPresent) { p.api_key_ref = null; }
      }
      await upsertProvider(p);
      status = t("common.saved");
      editing = null;
    } catch (e) { status = errorMessage(e); }
  }
  async function remove(p: ProviderConfig) {
    try { await deleteProvider(p.id); if (p.api_key_ref) await deleteSecret(p.api_key_ref); status = t("common.saved"); }
    catch (e) { status = errorMessage(e); }
  }
  function modelColumn(p: ProviderConfig): string {
    if (p.kind === "native") return p.native_id ?? "";
    if (p.kind === "embedded") return p.model_id ?? baseName(p.model_path);
    return p.model ?? "";
  }
  async function probeText(p: ProviderConfig): Promise<string> {
    try { return describeProbe(await probeProvider(p), t); }
    catch (e) { return `${t("providers.probe_failed")}: ${errorMessage(e)}`; }
  }
  async function runProbe() {
    if (!editing) return;
    probe = "...";
    probe = await probeText(editing);
  }
  async function probeRow(p: ProviderConfig) {
    rowProbes = { ...rowProbes, [p.id]: "..." };
    rowProbes = { ...rowProbes, [p.id]: await probeText(p) };
  }
</script>

<h2>{t("nav.providers")}</h2>
<table>
  <thead><tr><th>{t("providers.label")}</th><th>{t("providers.kind")}</th><th>{t("providers.model")}</th><th></th></tr></thead>
  <tbody>
    {#each config.value?.providers ?? [] as p}
      <tr><td>{p.label}</td><td>{t("providers.kind." + p.kind)}</td><td>{modelColumn(p)}</td>
        <td>
          <button onclick={() => edit(p)}>{t("common.edit")}</button>
          <button onclick={() => probeRow(p)}>{t("providers.probe")}</button>
          <button onclick={() => remove(p)}>{t("common.remove")}</button>
          {#if rowProbes[p.id]}<span class="muted">{rowProbes[p.id]}</span>{/if}
        </td></tr>
    {/each}
  </tbody>
</table>
<button onclick={() => edit(blankFor("http"))}>{t("providers.add")}</button>

{#if editing}
<section class="form">
  <label>{t("providers.label")} <input bind:value={editing.label} /></label>
  <label>{t("providers.kind")}
    <select value={editing.kind} onchange={(e) => setKind(e.currentTarget.value as ProviderKind)}>
      {#each ["http", "embedded", "native"] as k}<option value={k}>{t("providers.kind." + k)}</option>{/each}
    </select>
  </label>
  {#if editing.kind === "http"}
    <label>{t("providers.base_url")} <input bind:value={editing.base_url} placeholder="http://localhost:11434/v1" /></label>
    <label>{t("providers.model")} <input bind:value={editing.model} /></label>
    <label>{t("providers.api_key")} <input type="password" bind:value={apiKey} placeholder={keyPresent ? t("providers.key_present") : ""} /></label>
  {:else if editing.kind === "embedded"}
    <label>{t("providers.runtime")}
      <select value={editing.runtime} onchange={(e) => setRuntime(e.currentTarget.value as Runtime)}>
        {#each ["whisper", "llama"] as r}<option value={r}>{t("providers.runtime." + r)}</option>{/each}
      </select>
    </label>
    <label>{t("providers.model_select")}
      <select value={selectedModel} onchange={(e) => setModel(e.currentTarget.value)}>
        <option value=""></option>
        {#each candidates as m}<option value={m.custom ? `custom:${m.path}` : m.id}>{m.custom ? baseName(m.path) : m.id}</option>{/each}
      </select>
    </label>
    {#if candidates.length === 0}<p class="muted">{t("providers.no_models_hint")}</p>{:else if missingModel}<p class="muted">{t("providers.model_required")}</p>{/if}
  {:else}
    <label>{t("providers.native_id")}
      <select value={editing.native_id} onchange={(e) => setNativeId(e.currentTarget.value)}>
        <option value={NATIVE_DICTATION}>{t("providers.native.apple_dictation")}</option>
        <option value={NATIVE_FM}>{t("providers.native.apple_foundation_models")}</option>
      </select>
    </label>
    <div class="row">
      <span>{t("providers.native.status")}: {native ? t("providers.native." + native.status) : "..."}{native?.reason ? ` (${native.reason})` : ""}</span>
      {#if editing.native_id === NATIVE_DICTATION && native?.status === "needs_download"}
        <button onclick={installLanguage} disabled={installing}>{installing ? `${t("providers.installing")} ${Math.round(installFraction * 100)}%` : t("providers.install_language")}</button>
      {/if}
    </div>
  {/if}
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
    <button class="primary" onclick={save} disabled={missingModel}>{t("common.save")}</button>
    <button onclick={() => (editing = null)}>{t("common.cancel")}</button>
  </div>
</section>
{/if}
<p class="status">{status}</p>
