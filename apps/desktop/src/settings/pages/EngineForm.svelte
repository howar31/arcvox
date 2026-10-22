<script lang="ts">
  import { onMount, tick } from "svelte";
  import { deleteProvider, deleteSecret, errorMessage, hasSecret, probeProvider, setSecret, upsertProvider, type ProbeResult, type ProviderConfig } from "../../lib/api";
  import { blankHttp, freshId, type Role } from "../../lib/engines";
  import { t } from "../../lib/i18n";
  import { usedBy } from "../../lib/labels";
  import { describeFailure } from "../../lib/providerForm";
  import { config } from "../../lib/store.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  // Only a network service is made by hand: local models and the platform's
  // services bring their own engines. The role comes from the page's tab.
  // Two steps: the address and the key are tried first; the model is then
  // picked from what the service lists, or typed when it lists none.
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
  // An existing engine opens on the model step; a new one on the address step.
  let step = $state<"connect" | "model">(isNew ? "connect" : "model");
  let probe = $state<{ state: "wait" | "ok" | "bad"; text: string } | null>(null);
  // What the service listed at the last connection; null before any.
  let listed = $state<string[] | null>(null);
  let query = $state("");
  // "typed" means the model is written by hand rather than picked from the list.
  // svelte-ignore state_referenced_locally
  let typed = $state(false);
  // svelte-ignore state_referenced_locally
  let typedModel = $state(initial?.model ?? "");
  let busy = $state(false);
  // Deleting asks first, in place of the form's buttons.
  let confirmDelete = $state(false);
  let keepButton: HTMLButtonElement | undefined = $state();
  // Esc or the backdrop with unsaved edits asks too, in the same place.
  let confirmDiscard = $state(false);
  // svelte-ignore state_referenced_locally
  const pristine = JSON.stringify($state.snapshot(editing));
  const dirty = $derived(apiKey !== "" || JSON.stringify($state.snapshot(editing)) !== pristine);

  const hasList = $derived(!!listed && listed.length > 0);
  const shown = $derived.by(() => {
    if (!listed) return [];
    const q = query.trim().toLowerCase();
    return q ? listed.filter((m) => m.toLowerCase().includes(q)) : listed;
  });
  /** The model the form would store: the picked one, or the typed one. */
  const chosenModel = $derived(hasList && !typed ? (editing.model ?? "") : typedModel.trim());
  // With a list, a model is always named; without one it may be empty.
  const canSave = $derived(!!editing.base_url?.trim() && keyKnown && (!hasList || chosenModel !== ""));
  const host = $derived.by(() => {
    try { return new URL(editing.base_url ?? "").host; } catch { return editing.base_url ?? ""; }
  });

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
    if (!isNew) connect(false);
  });

  /** The engine as the connection and the save see it, with the key typed now. */
  function current(): ProviderConfig {
    const p = $state.snapshot(editing) as ProviderConfig;
    p.base_url = p.base_url?.trim() || null;
    return p;
  }

  /** Asks the service for its models. `advance` moves to the model step when it answers. */
  async function connect(advance: boolean) {
    probe = { state: "wait", text: t("engines.connecting") };
    let r: ProbeResult;
    try {
      // A key typed now is stored before the connection so the service sees it.
      const p = current();
      if (apiKey.trim()) {
        p.api_key_ref = p.id || freshId(p.label || p.base_url || "engine", []);
        await setSecret(p.api_key_ref, apiKey);
        editing.api_key_ref = p.api_key_ref;
        keyPresent = true;
        apiKey = "";
      }
      r = await probeProvider(p);
    } catch (e) {
      probe = { state: "bad", text: errorMessage(e) };
      return;
    }
    probe = { state: r.ok ? "ok" : "bad", text: r.ok ? t("engines.connected") : describeFailure(r, t) };
    if (!r.ok) return;
    listed = r.models;
    // A stored model the service does not list is kept as typed.
    const stored = editing.model ?? "";
    typed = r.models.length === 0 || (stored !== "" && !r.models.includes(stored));
    if (typed) typedModel = stored;
    else if (!stored) editing.model = r.models[0];
    if (advance) step = "model";
  }

  /** Goes to the model step without a connection: the model is typed. */
  function skipConnect() {
    listed = [];
    typed = true;
    step = "model";
  }

  async function save() {
    const p = current();
    p.model = chosenModel || null;
    p.label = p.label.trim() || p.model || p.base_url || t("engines.external");
    if (!p.id) p.id = freshId(p.label, (config.value?.providers ?? []).map((x) => x.id));
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
</script>

<Modal bind:open title={confirmDiscard ? t("common.discard_title") : confirmDelete ? t("engines.delete_title") : isNew ? t("engines.add_external") : t("engines.edit_title")} {beforeclose}>
  {#if confirmDiscard}
    <p>{t("engines.discard_body")}</p>
  {:else if confirmDelete}
    <p>{t("engines.delete_body", { name: initial?.label ?? "" })}</p>
  {:else}
    <div class="form">
      <ol class="steps" aria-label={t("engines.steps")}>
        <li><button type="button" class:on={step === "connect"} onclick={() => (step = "connect")}><i>1</i>{t("engines.step_connect")}</button></li>
        <li><button type="button" class:on={step === "model"} disabled={listed === null} onclick={() => (step = "model")}><i>2</i>{t("engines.step_model")}</button></li>
      </ol>
      {#if step === "connect"}
        <div class="fields">
          <label for="f-url-{uid}">{t("engines.base_url")}<span class="req">{t("common.required")}</span></label>
          <div>
            <input id="f-url-{uid}" type="url" bind:value={editing.base_url} placeholder="http://localhost:11434/v1" />
            <small>{t("engines.base_url_help")}</small>
          </div>
          <label for="f-key-{uid}">{t("engines.api_key")}</label>
          <div>
            <input id="f-key-{uid}" type="password" bind:value={apiKey} placeholder={keyPresent ? t("engines.key_present") : ""} autocomplete="off" />
            <small>{t("engines.api_key_help")}</small>
          </div>
        </div>
        {#if probe && probe.state !== "ok"}<p class="probe {probe.state}" role="status">{#if probe.state === "bad"}<b>{t("engines.not_connected")}</b>{/if}{probe.text}</p>{/if}
      {:else}
        <p class="status" role="status">
          {#if probe?.state === "wait"}
            {probe.text}
          {:else if probe?.state === "ok"}
            <span class="ok">{t("engines.connected")}</span>{hasList ? t("engines.connected_list", { host, n: String(listed?.length ?? 0) }) : t("engines.connected_no_list", { host })}
          {:else}
            <span class="bad">{t("engines.not_connected")}</span>{probe?.text ?? ""}
          {/if}
          <button type="button" class="btn quiet small" onclick={() => connect(false)} disabled={busy || probe?.state === "wait"}>{t("engines.reconnect")}</button>
        </p>
        <div class="fields">
          <span class="lab">{t("engines.model")}{#if hasList}<span class="req">{t("common.required")}</span>{/if}</span>
          <div>
            {#if hasList && listed}
              {#if listed.length > 8}
                <input type="search" class="search" bind:value={query} placeholder={t("engines.search_models")} aria-label={t("engines.search_models")} />
              {/if}
              <div class="models" role="radiogroup" aria-label={t("engines.model")}>
                {#each shown as m (m)}
                  <label class="model" class:on={!typed && editing.model === m}>
                    <input type="radio" name="model-{uid}" value={m} checked={!typed && editing.model === m} onchange={() => { typed = false; editing.model = m; }} />
                    <span class="mname">{m}</span>
                  </label>
                {:else}
                  <p class="none">{t("engines.no_model_match")}</p>
                {/each}
              </div>
              <label class="model other" class:on={typed}>
                <input type="radio" name="model-{uid}" value="" checked={typed} onchange={() => { typed = true; }} />
                <span class="mname">{t("engines.model_other")}</span>
                {#if typed}<input type="text" class="typed" bind:value={typedModel} aria-label={t("engines.model")} />{/if}
              </label>
            {:else}
              <input type="text" bind:value={typedModel} aria-label={t("engines.model")} />
              <small>{t("engines.model_type_help")}</small>
            {/if}
          </div>
          <label for="f-label-{uid}">{t("engines.label")}</label>
          <div>
            <input id="f-label-{uid}" type="text" bind:value={editing.label} placeholder={chosenModel || host} />
            <small>{t("engines.label_help")}</small>
          </div>
        </div>
      {/if}
    </div>
  {/if}

  {#snippet actions()}
    {#if confirmDiscard}
      <button type="button" class="btn" bind:this={keepButton} onclick={() => (confirmDiscard = false)}>{t("common.cancel")}</button>
      <button type="button" class="btn danger" onclick={() => (open = false)}>{t("common.discard")}</button>
    {:else if confirmDelete}
      <button type="button" class="btn" bind:this={keepButton} onclick={() => (confirmDelete = false)}>{t("common.cancel")}</button>
      <button type="button" class="btn danger" onclick={remove} disabled={busy}>{t("engines.delete")}</button>
    {:else if step === "connect"}
      {#if probe?.state === "bad"}<button type="button" class="btn quiet lead" onclick={skipConnect}>{t("engines.continue_anyway")}</button>{/if}
      <button type="button" class="btn" onclick={() => (open = false)}>{t("common.cancel")}</button>
      <button type="button" class="btn primary" onclick={() => connect(true)} disabled={!editing.base_url?.trim() || probe?.state === "wait"}>{t("engines.connect")}</button>
    {:else}
      {#if !isNew}<button type="button" class="btn quiet danger lead" onclick={askDelete} disabled={busy}>{t("engines.delete")}</button>{/if}
      <button type="button" class="btn quiet" class:lead={isNew} onclick={() => (step = "connect")} disabled={busy}><Icon name="chevron-left" size={14} />{t(isNew ? "engines.back" : "engines.change_connection")}</button>
      <button type="button" class="btn" onclick={() => (open = false)}>{t("common.cancel")}</button>
      <button type="button" class="btn primary" onclick={save} disabled={busy || !canSave}>{t("common.save")}</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .form { display: flex; flex-direction: column; gap: 14px; margin-top: 4px; color: var(--ink); }
  .steps { display: flex; align-items: center; gap: 14px; margin: 0; padding: 0; list-style: none; font-size: var(--text-sm); color: var(--ink-3); }
  .steps li { display: inline-flex; align-items: center; }
  .steps button { display: inline-flex; align-items: center; gap: 7px; padding: 2px 6px 2px 2px; border: 0; border-radius: 13px; background: none; color: var(--ink-3); font: inherit; font-size: var(--text-sm); cursor: default; }
  .steps button:not([disabled]):hover { background: var(--sunken); color: var(--ink); }
  .steps button.on { color: var(--ink); font-weight: 600; }
  .steps li + li::before { content: "›"; margin: 0 8px; color: var(--ink-3); }
  .steps i { display: inline-grid; place-items: center; width: 22px; height: 22px; border-radius: 50%; background: var(--sunken); font-style: normal; font-size: var(--text-xs); font-weight: 600; color: var(--ink-2); }
  .steps button.on i { background: var(--accent); color: var(--accent-ink); }
  .fields { display: grid; grid-template-columns: minmax(96px, max-content) minmax(0, 1fr); gap: 14px; align-items: start; }
  .fields > label, .fields > .lab { color: var(--ink-2); padding-top: 7px; white-space: nowrap; }
  .fields input[type="url"], .fields input[type="password"], .fields input[type="text"], .fields .search { width: 100%; }
  .fields small { display: block; margin: 5px 2px 0; color: var(--ink-2); font-size: var(--text-xs); line-height: 1.45; }
  .req { margin-left: 6px; padding: 1px 6px; border-radius: 6px; background: var(--accent-soft); color: var(--accent); font-size: 11px; font-weight: 600; vertical-align: 2px; }
  .probe { display: flex; gap: 8px; margin: 0; font-size: var(--text-sm); color: var(--ink-2); overflow-wrap: anywhere; }
  .probe.bad b { color: var(--live); font-weight: 600; }
  .status { display: flex; align-items: center; gap: 8px; margin: 0; font-size: var(--text-sm); color: var(--ink-2); }
  .status .ok { color: var(--ok); font-weight: 600; }
  .status .bad { color: var(--live); font-weight: 600; }
  .status .small { margin-left: auto; height: 28px; padding: 0 8px; font-size: var(--text-sm); }
  .search { margin-bottom: 6px; }
  .models { border: 1px solid var(--line); border-radius: 12px; overflow: hidden; max-height: 220px; overflow-y: auto; }
  .model { display: flex; align-items: center; gap: 10px; padding: 8px 12px; border-top: 1px solid var(--line); font-size: var(--text-control); cursor: default; }
  .model:first-child { border-top: 0; }
  .model.other { margin-top: 6px; border: 1px solid var(--line); border-radius: 12px; }
  .model:hover { background: var(--sunken); }
  .model.on { background: var(--accent-soft); }
  .model input[type="radio"] { margin: 0; }
  .mname { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .model .typed { flex: 1; min-width: 0; height: 30px; }
  .none { margin: 0; padding: 12px; color: var(--ink-2); font-size: var(--text-sm); }
  .lead { margin-right: auto; margin-left: -8px; }
</style>
