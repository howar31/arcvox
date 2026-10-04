<script lang="ts">
  import { onMount, tick } from "svelte";
  import {
    deleteProvider, deleteSecret, errorMessage, hasSecret, nativeInstallLanguage, nativeStatus, onNativeInstallProgress, probeProvider, setSecret,
    upsertProvider, type InstalledModel, type ManifestView, type NativeStatusView, type ProviderConfig, type ProviderKind, type Runtime,
  } from "../../lib/api";
  import { blankForRole, freshId, hasRole, type Role } from "../../lib/engines";
  import { t } from "../../lib/i18n";
  import { defaultProfile, modelLabel, usedBy } from "../../lib/labels";
  import { baseName, capsForNative, capsForRuntime, describeProbe, NATIVE_DICTATION, NATIVE_FM } from "../../lib/providerForm";
  import { config } from "../../lib/store.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import Ring from "../../lib/ui/Ring.svelte";
  import Segmented from "../../lib/ui/Segmented.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  let { open = $bindable(false), initial, role, installed, manifest }: {
    open?: boolean;
    /** The engine to edit; null adds a new one. */
    initial: ProviderConfig | null;
    role: Role;
    installed: InstalledModel[];
    manifest: ManifestView | null;
  } = $props();

  // The form works on a copy; nothing is stored until Save.
  // svelte-ignore state_referenced_locally
  const isNew = initial === null;
  // svelte-ignore state_referenced_locally
  let editing = $state<ProviderConfig>(initial ? structuredClone($state.snapshot(initial)) : blankForRole("embedded", role));
  let apiKey = $state("");
  let keyPresent = $state(false);
  // Save waits until the stored key's presence is known, so a slow check cannot drop the key.
  // svelte-ignore state_referenced_locally
  let keyKnown = $state(!initial?.api_key_ref);
  let probe = $state<{ state: "wait" | "ok" | "bad"; text: string } | null>(null);
  let native = $state<NativeStatusView | null>(null);
  let installing = $state(false);
  let installFraction = $state(0);
  let busy = $state(false);
  // Deleting asks first, in place of the form's buttons.
  let confirmDelete = $state(false);
  let keepButton: HTMLButtonElement | undefined = $state();
  // Esc or the backdrop with unsaved edits asks too, in the same place.
  let confirmDiscard = $state(false);
  // svelte-ignore state_referenced_locally
  const pristine = JSON.stringify($state.snapshot(editing));
  const dirty = $derived(apiKey !== "" || JSON.stringify($state.snapshot(editing)) !== pristine);

  function beforeclose(): boolean {
    if (confirmDiscard || confirmDelete || busy || !dirty) return !busy;
    confirmDiscard = true;
    tick().then(() => keepButton?.focus());
    return false;
  }

  async function askDelete() {
    confirmDelete = true;
    await tick();
    keepButton?.focus();
  }

  const uid = Math.random().toString(36).slice(2);
  const defaultHint = $derived(defaultProfile(config.value)?.language_hint ?? null);
  const candidates = $derived(installed.filter((m) => m.runtime === editing.runtime));
  // The select value encodes either a catalogue id or `custom:<path>`.
  const selectedModel = $derived(editing.model_path ? `custom:${editing.model_path}` : (editing.model_id ?? ""));
  const missingModel = $derived(editing.kind === "embedded" && !editing.model_id && !editing.model_path);
  const noRole = $derived(!hasRole(editing));
  const kinds = $derived(
    (["embedded", "native", "http"] as const).map((k) => ({ value: k as ProviderKind, label: t("engines.kind." + k) })),
  );

  const runtimes = $derived(
    (["whisper", "llama"] as const).map((r) => ({ value: r as Runtime, label: t("engines.runtime." + r) })),
  );

  onMount(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    onNativeInstallProgress((f) => (installFraction = f)).then((u) => { if (disposed) u(); else unlisten = u; });
    if (editing.kind === "native") refreshNative();
    if (editing.api_key_ref) {
      hasSecret(editing.api_key_ref)
        .then((v) => { keyPresent = v; })
        .catch(() => { keyPresent = false; })
        .finally(() => { keyKnown = true; });
    }
    return () => { disposed = true; unlisten?.(); };
  });

  // Only the most recent request may update the status line.
  let nativeRequest = 0;
  async function refreshNative() {
    const req = ++nativeRequest;
    if (editing.kind !== "native" || !editing.native_id) { native = null; return; }
    native = null;
    let next: NativeStatusView;
    try { next = await nativeStatus(editing.native_id, defaultHint); } catch (e) { next = { status: "failed", reason: errorMessage(e) }; }
    if (req === nativeRequest) native = next;
  }
  const STATUSES = ["ready", "needs_download", "locale_unsupported", "os_too_old", "not_eligible", "intelligence_off", "model_not_ready", "failed"];
  const installError = (e: unknown) => {
    const m = errorMessage(e);
    return STATUSES.includes(m) ? t("engines.native." + m) : m;
  };

  function setKind(kind: ProviderKind) {
    if (kind === editing.kind) return;
    // Back on the kind the engine was saved with, its stored fields return (the key reference included,
    // so a stored key stays attached); otherwise the new kind's blank for this role. The name is kept.
    const fresh = initial && kind === initial.kind ? (structuredClone($state.snapshot(initial)) as ProviderConfig) : blankForRole(kind, role);
    fresh.id = editing.id;
    fresh.label = editing.label;
    editing = fresh;
    probe = null;
    refreshNative();
  }
  function setRuntime(r: Runtime) {
    editing.runtime = r;
    editing.capabilities = capsForRuntime(r);
    editing.model_id = null;
    editing.model_path = null;
  }
  function setNativeId(id: string) {
    editing.native_id = id;
    editing.capabilities = capsForNative(id);
    refreshNative();
  }
  function setModel(value: string) {
    if (value.startsWith("custom:")) { editing.model_path = value.slice("custom:".length); editing.model_id = null; }
    else { editing.model_id = value || null; editing.model_path = null; }
    const m = installed.find((x) => (x.custom ? `custom:${x.path}` : x.id) === value);
    if (m && !editing.label.trim()) editing.label = modelLabel(manifest, m);
  }

  async function installLanguage() {
    installing = true;
    installFraction = 0;
    try { await nativeInstallLanguage(defaultHint); }
    catch (e) { toast.error(installError(e)); }
    installing = false;
    await refreshNative();
  }

  /** A name for an engine saved without one. */
  function fallbackLabel(p: ProviderConfig): string {
    if (p.kind === "native") return t("engines.native." + (p.native_id === NATIVE_FM ? "apple_foundation_models" : "apple_dictation"));
    if (p.kind === "http") return p.model || p.base_url || t("engines.kind.http");
    const m = installed.find((x) => (p.model_path ? x.path === p.model_path : x.id === p.model_id));
    return m ? modelLabel(manifest, m) : (p.model_id ?? baseName(p.model_path));
  }

  async function save() {
    const p = $state.snapshot(editing) as ProviderConfig;
    p.label = p.label.trim() || fallbackLabel(p);
    if (!p.id) p.id = freshId(p.label, (config.value?.providers ?? []).map((x) => x.id));
    p.model = p.model?.trim() || null;
    p.base_url = p.base_url?.trim() || null;
    busy = true;
    try {
      if (p.kind === "http") {
        if (apiKey.trim()) { p.api_key_ref = p.id; await setSecret(p.id, apiKey); }
        else if (!keyPresent) { p.api_key_ref = null; }
      }
      await upsertProvider(p);
      // A key the engine no longer refers to (kind changed, or stored under a new reference) is removed.
      const oldRef = initial?.api_key_ref;
      if (oldRef && oldRef !== p.api_key_ref) await deleteSecret(oldRef).catch(() => {});
      toast.ok(t("common.saved"));
      open = false;
    } catch (e) { toast.error(errorMessage(e)); }
    busy = false;
  }

  async function remove() {
    if (!initial) return;
    confirmDelete = false;
    // An engine a profile still uses cannot be deleted; say which profiles, in the interface language.
    const users = usedBy(config.value, initial.id);
    if (users.length > 0) {
      toast.error(t("engines.delete_in_use", { names: users.map((u) => u.name).join(t("home.list_sep")) }));
      return;
    }
    busy = true;
    try {
      // The core refuses to delete an engine a profile still uses; its message explains which.
      await deleteProvider(initial.id);
      if (initial.api_key_ref) await deleteSecret(initial.api_key_ref);
      toast.ok(t("engines.deleted"));
      open = false;
    } catch (e) { toast.error(errorMessage(e)); }
    busy = false;
  }

  async function runProbe() {
    probe = { state: "wait", text: t("engines.testing") };
    try {
      const r = await probeProvider($state.snapshot(editing) as ProviderConfig);
      probe = { state: r.ok ? "ok" : "bad", text: describeProbe(r, t) };
    } catch (e) {
      probe = { state: "bad", text: `${t("engines.probe_failed")}: ${errorMessage(e)}` };
    }
  }
</script>

<Modal bind:open title={confirmDiscard ? t("common.discard_title") : confirmDelete ? t("engines.delete_title") : isNew ? t("engines.add") : t("engines.edit_title")} {beforeclose}>
  {#if confirmDiscard}
    <p>{t("engines.discard_body")}</p>
  {:else if confirmDelete}
    <p>{t("engines.delete_body", { name: initial?.label ?? "" })}</p>
  {:else}
    <div class="form">
      <div><Segmented options={kinds} bind:value={() => editing.kind, (k) => setKind(k)} label={t("engines.kind")} /></div>

      <div class="fields">
        <label for="f-label-{uid}">{t("engines.label")}</label>
        <input id="f-label-{uid}" type="text" bind:value={editing.label} />

        {#if editing.kind === "embedded"}
          <label for="f-model-{uid}">{t("engines.model")}</label>
          <select id="f-model-{uid}" value={selectedModel} onchange={(e) => setModel(e.currentTarget.value)}>
            <option value=""></option>
            {#each candidates as m (m.path)}<option value={m.custom ? `custom:${m.path}` : m.id}>{modelLabel(manifest, m)}</option>{/each}
          </select>
          {#if candidates.length === 0}<p class="note">{t("engines.no_models_hint")}</p>
          {:else if missingModel}<p class="note">{t("engines.model_required")}</p>{/if}
        {:else if editing.kind === "native"}
          <label for="f-native-{uid}">{t("engines.native_id")}</label>
          <select id="f-native-{uid}" value={editing.native_id} onchange={(e) => setNativeId(e.currentTarget.value)}>
            <option value={NATIVE_DICTATION}>{t("engines.native.apple_dictation")}</option>
            <option value={NATIVE_FM}>{t("engines.native.apple_foundation_models")}</option>
          </select>
          <span class="lbl">{t("engines.native.status")}</span>
          <div class="status">
            {#if native}
              <Ring tone={native.status === "ready" ? "ok" : "off"} size="sm" />
              <span class="grow">{t("engines.native." + native.status)}{native.reason ? ` (${native.reason})` : ""}</span>
            {:else}
              <span class="grow">{t("common.loading")}</span>
            {/if}
            {#if editing.native_id === NATIVE_DICTATION && native?.status === "needs_download"}
              <button type="button" class="btn" onclick={installLanguage} disabled={installing}>
                {installing ? t("engines.installing", { percent: `${Math.round(installFraction * 100)}%` }) : t("engines.install_language")}
              </button>
            {/if}
          </div>
        {:else}
          <label for="f-url-{uid}">{t("engines.base_url")}</label>
          <input id="f-url-{uid}" type="url" bind:value={editing.base_url} placeholder="http://localhost:11434/v1" />
          <label for="f-remote-model-{uid}">{t("engines.model")}</label>
          <input id="f-remote-model-{uid}" type="text" bind:value={editing.model} />
          <label for="f-key-{uid}">{t("engines.api_key")}</label>
          <input id="f-key-{uid}" type="password" bind:value={apiKey} placeholder={keyPresent ? t("engines.key_present") : ""} autocomplete="off" />
        {/if}
      </div>

      <details>
        <summary><span class="chev"><Icon name="chevron-right" size={14} /></span>{t("engines.advanced")}</summary>
        <div class="fields adv">
          {#if editing.kind === "embedded"}
            <span class="lbl">{t("engines.runtime")}</span>
            <div><Segmented options={runtimes} bind:value={() => editing.runtime ?? "whisper", (r) => setRuntime(r)} label={t("engines.runtime")} /></div>
          {/if}
          <span class="lbl">{t("engines.capabilities")}</span>
          <div class="caps">
            <label><input type="checkbox" bind:checked={editing.capabilities.transcribe} /> {t("engines.cap_transcribe")}</label>
            <label><input type="checkbox" bind:checked={editing.capabilities.chat} /> {t("engines.cap_chat")}</label>
            <label><input type="checkbox" bind:checked={editing.capabilities.language_hint} /> {t("engines.cap_language_hint")}</label>
            <label><input type="checkbox" bind:checked={editing.capabilities.hotwords} /> {t("engines.cap_hotwords")}</label>
          </div>
        </div>
      </details>
      {#if noRole}<p class="note-bad" role="alert">{t("engines.no_role_help")}</p>{/if}

      {#if probe}<p class="probe {probe.state}" role="status">{probe.text}</p>{/if}
    </div>
  {/if}

  {#snippet actions()}
    {#if confirmDiscard}
      <button type="button" class="btn" bind:this={keepButton} onclick={() => (confirmDiscard = false)}>{t("common.cancel")}</button>
      <button type="button" class="btn danger" onclick={() => (open = false)}>{t("common.discard")}</button>
    {:else if confirmDelete}
      <button type="button" class="btn" bind:this={keepButton} onclick={() => (confirmDelete = false)}>{t("common.cancel")}</button>
      <button type="button" class="btn danger" onclick={remove} disabled={busy}>{t("engines.delete")}</button>
    {:else}
      {#if !isNew}<button type="button" class="btn quiet danger delete" onclick={askDelete} disabled={busy}>{t("engines.delete")}</button>{/if}
      <button type="button" class="btn" onclick={runProbe} disabled={busy || probe?.state === "wait"}>{t("engines.test")}</button>
      <button type="button" class="btn" onclick={() => (open = false)}>{t("common.cancel")}</button>
      <button type="button" class="btn primary" onclick={save} disabled={busy || missingModel || noRole || !keyKnown}>{t("common.save")}</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .form { display: flex; flex-direction: column; gap: 14px; margin-top: 8px; color: var(--ink); }
  .fields { display: grid; grid-template-columns: 96px minmax(0, 1fr); gap: 12px 14px; align-items: center; }
  .fields > label, .lbl { color: var(--ink-2); }
  .fields input:not([type="checkbox"]), .fields select { width: 100%; }
  .note { grid-column: 2; margin: -4px 0 0; font-size: var(--text-sm); color: var(--ink-2); }
  .status { display: flex; align-items: center; gap: 8px; min-height: var(--control); min-width: 0; }
  .grow { flex: 1; min-width: 0; }
  details { border-top: 1px solid var(--line); padding-top: 10px; }
  summary { display: flex; align-items: center; gap: 6px; color: var(--ink-2); cursor: default; width: max-content; list-style: none; }
  summary::-webkit-details-marker { display: none; }
  .chev { display: flex; transition: transform .15s; }
  details[open] .chev { transform: rotate(90deg); }
  .adv { margin-top: 10px; align-items: start; }
  .adv > .lbl { line-height: var(--control); }
  .caps { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 4px 12px; }
  .caps label { display: flex; align-items: center; gap: 8px; line-height: 30px; }
  .probe { margin: 0; font-size: var(--text-sm); color: var(--ink-2); overflow-wrap: anywhere; }
  .probe.ok { color: var(--ok); }
  .probe.bad { color: var(--live); }
  .note-bad { margin: -4px 0 0; font-size: var(--text-sm); color: var(--live); }
  .delete { margin-right: auto; margin-left: -8px; }
</style>
