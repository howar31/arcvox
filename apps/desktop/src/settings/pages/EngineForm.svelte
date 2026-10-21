<script lang="ts">
  import { onMount, tick } from "svelte";
  import { deleteProvider, deleteSecret, errorMessage, hasSecret, probeProvider, setSecret, upsertProvider, type ProviderConfig } from "../../lib/api";
  import { blankHttp, freshId, type Role } from "../../lib/engines";
  import { t } from "../../lib/i18n";
  import { usedBy } from "../../lib/labels";
  import { describeProbe } from "../../lib/providerForm";
  import { config } from "../../lib/store.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  // Only a network service is made by hand: local models and the platform's
  // services bring their own engines. The role comes from the page's tab.
  let { open = $bindable(false), initial, role }: {
    open?: boolean;
    /** The engine to edit; null adds a new one. */
    initial: ProviderConfig | null;
    role: Role;
  } = $props();

  // The form works on a copy; nothing is stored until Save.
  // svelte-ignore state_referenced_locally
  const isNew = initial === null;
  // svelte-ignore state_referenced_locally
  let editing = $state<ProviderConfig>(initial ? structuredClone($state.snapshot(initial)) : blankHttp(role));
  let apiKey = $state("");
  let keyPresent = $state(false);
  // Save waits until the stored key's presence is known, so a slow check cannot drop the key.
  // svelte-ignore state_referenced_locally
  let keyKnown = $state(!initial?.api_key_ref);
  let probe = $state<{ state: "wait" | "ok" | "bad"; text: string } | null>(null);
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

  onMount(() => {
    if (editing.api_key_ref) {
      hasSecret(editing.api_key_ref)
        .then((v) => { keyPresent = v; })
        .catch(() => { keyPresent = false; })
        .finally(() => { keyKnown = true; });
    }
  });

  async function save() {
    const p = $state.snapshot(editing) as ProviderConfig;
    p.label = p.label.trim() || p.model || p.base_url || t("engines.external");
    if (!p.id) p.id = freshId(p.label, (config.value?.providers ?? []).map((x) => x.id));
    p.model = p.model?.trim() || null;
    p.base_url = p.base_url?.trim() || null;
    busy = true;
    try {
      if (apiKey.trim()) { p.api_key_ref = p.id; await setSecret(p.id, apiKey); }
      else if (!keyPresent) { p.api_key_ref = null; }
      config.value = await upsertProvider(p);
      // A key stored under an earlier reference is removed.
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

<Modal bind:open title={confirmDiscard ? t("common.discard_title") : confirmDelete ? t("engines.delete_title") : isNew ? t("engines.add_external") : t("engines.edit_title")} {beforeclose}>
  {#if confirmDiscard}
    <p>{t("engines.discard_body")}</p>
  {:else if confirmDelete}
    <p>{t("engines.delete_body", { name: initial?.label ?? "" })}</p>
  {:else}
    <div class="form">
      <div class="fields">
        <label for="f-label-{uid}">{t("engines.label")}</label>
        <input id="f-label-{uid}" type="text" bind:value={editing.label} />
        <label for="f-url-{uid}">{t("engines.base_url")}</label>
        <input id="f-url-{uid}" type="url" bind:value={editing.base_url} placeholder="http://localhost:11434/v1" />
        <label for="f-remote-model-{uid}">{t("engines.model")}</label>
        <input id="f-remote-model-{uid}" type="text" bind:value={editing.model} />
        <label for="f-key-{uid}">{t("engines.api_key")}</label>
        <input id="f-key-{uid}" type="password" bind:value={apiKey} placeholder={keyPresent ? t("engines.key_present") : ""} autocomplete="off" />
      </div>
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
      <button type="button" class="btn primary" onclick={save} disabled={busy || !keyKnown}>{t("common.save")}</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .form { display: flex; flex-direction: column; gap: 14px; margin-top: 8px; color: var(--ink); }
  .fields { display: grid; grid-template-columns: 96px minmax(0, 1fr); gap: 12px 14px; align-items: center; }
  .fields > label { color: var(--ink-2); }
  .fields input { width: 100%; }
  .probe { margin: 0; font-size: var(--text-sm); color: var(--ink-2); overflow-wrap: anywhere; }
  .probe.ok { color: var(--ok); }
  .probe.bad { color: var(--live); }
  .delete { margin-right: auto; margin-left: -8px; }
</style>
