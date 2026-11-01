<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { deleteProvider, deleteSecret, errorMessage, hasSecret, listServices, openServiceKeyPage, probeProvider, putEngineCheck, scanLocalServices, setSecret, upsertProvider, verifyProvider, type EngineCheck, type LocalService, type ProbeResult, type ProviderConfig, type ServicePreset } from "../../lib/api";
  import { blankHttp, freshId, type Role } from "../../lib/engines";
  import { t } from "../../lib/i18n";
  import { usedBy } from "../../lib/labels";
  import { ASR_CAPS, describeFailure, LLM_CAPS } from "../../lib/providerForm";
  import { checkKey, cloudFor, EFFORTS, localFor, localRows, otherRole, pickId, pickOf, servesOther, suggestedIn, takesEffort, type ServicePick } from "../../lib/serviceForm";
  import { config } from "../../lib/store.svelte";
  import Fold from "../../lib/ui/Fold.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import ServiceAnswer from "../../lib/ui/ServiceAnswer.svelte";
  import Switch from "../../lib/ui/Switch.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  // Only a network service is made by hand: local models and the platform's
  // services bring their own engines. The role comes from the page's tab.
  // Step one picks the service and connects to it; step two picks the model,
  // and a new engine for the other role from the same service when wanted.
  // An engine is added only after one real request to it passed.
  let { open = $bindable(false), initial, role }: {
    open?: boolean;
    /** The engine to edit; null adds a new one. */
    initial: ProviderConfig | null;
    role: Role;
  } = $props();

  // svelte-ignore state_referenced_locally
  const isNew = initial === null;
  // svelte-ignore state_referenced_locally
  const other = otherRole(role);
  // svelte-ignore state_referenced_locally
  let editing = $state<ProviderConfig>(initial ? structuredClone($state.snapshot(initial)) : blankHttp(role));
  let services = $state<ServicePreset[]>([]);
  let pick = $state<ServicePick>({ kind: "custom" });
  // svelte-ignore state_referenced_locally
  let customUrl = $state(initial && !initial.service ? (initial.base_url ?? "") : "");
  let found = $state<LocalService[] | null>(null);
  let scanning = $state(false);
  let apiKey = $state("");
  let keyPresent = $state(false);
  // svelte-ignore state_referenced_locally
  let keyKnown = $state(!initial?.api_key_ref);
  // A key typed for a new engine is kept under this one reference while the form is open; each engine
  // stores its own copy on save. One fixed name, so a form ended by quitting leaves at most one behind,
  // and the next form replaces it.
  const pendingRef = "pending-new-engine";
  let savedAll = false;
  // svelte-ignore state_referenced_locally
  let step = $state<"connect" | "model">(isNew ? "connect" : "model");
  let probe = $state<{ state: "wait" | "ok" | "bad"; text: string } | null>(null);
  let listed = $state<string[] | null>(null);

  type Sel = { model: string; typed: boolean; typedModel: string; label: string; effort: string | null; query: string };
  const blankSel = (): Sel => ({ model: "", typed: false, typedModel: "", label: "", effort: null, query: "" });
  // svelte-ignore state_referenced_locally
  let sel = $state<Record<Role, Sel>>({
    asr: blankSel(),
    llm: blankSel(),
  });
  // svelte-ignore state_referenced_locally
  if (initial) {
    sel[role].model = initial.model ?? "";
    sel[role].typedModel = initial.model ?? "";
    sel[role].label = initial.label;
    sel[role].effort = initial.reasoning_effort ?? null;
  }
  let alsoOn = $state(false);
  // Real requests that passed, by what they depended on; one is not sent again.
  let passed = $state<Record<string, EngineCheck>>({});
  // The last result shown under each group.
  let shown = $state<Partial<Record<Role, EngineCheck>>>({});
  let armed = $state(false);
  let armTimer: ReturnType<typeof setTimeout> | undefined;
  let busy = $state(false);
  let confirmDelete = $state(false);
  let keepButton: HTMLButtonElement | undefined = $state();
  let confirmDiscard = $state(false);
  // svelte-ignore state_referenced_locally
  const pristine = JSON.stringify($state.snapshot(editing));
  const dirty = $derived(apiKey !== "" || JSON.stringify($state.snapshot(editing)) !== pristine || (isNew && step === "model"));

  const preset = $derived(pickId(pick) === null ? undefined : services.find((s) => s.id === pickId(pick)));
  const isLocal = $derived(pick.kind === "local");
  const keyNeeded = $derived(pick.kind === "cloud" && !!preset?.key_required);
  const hasList = $derived(!!listed && listed.length > 0);
  const roles = $derived<Role[]>(isNew && alsoOn && servesOther(pick, services, role) ? [role, other] : [role]);
  const host = $derived.by(() => {
    try { return new URL(editing.base_url ?? "").host; } catch { return editing.base_url ?? ""; }
  });
  const serviceName = $derived(preset?.label ?? host);
  const canConnect = $derived.by(() => {
    if (pick.kind === "custom") return customUrl.trim() !== "";
    if (pick.kind === "local") return !!found?.find((f) => f.id === pickId(pick) && f.running);
    return !!preset && (!keyNeeded || keyPresent || apiKey.trim() !== "");
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

  onMount(async () => {
    if (editing.api_key_ref) {
      hasSecret(editing.api_key_ref)
        .then((v) => { keyPresent = v; })
        .catch(() => { keyPresent = false; })
        .finally(() => { keyKnown = true; });
    }
    try { services = await listServices(); } catch { services = []; }
    if (initial) pick = pickOf(initial, services);
    else {
      const first = cloudFor(services, role)[0];
      pick = first ? { kind: "cloud", id: first.id } : { kind: "custom" };
    }
    applyPick();
    if (pick.kind === "local") await scan();
    if (!isNew) connect(false);
  });

  /** Points the engine at what the first step picked. */
  function applyPick() {
    if (pick.kind === "custom") {
      editing.service = null;
      editing.base_url = customUrl.trim() || null;
      return;
    }
    const id = pickId(pick);
    editing.service = id;
    if (pick.kind === "cloud") editing.base_url = preset?.base_url ?? null;
    else editing.base_url = found?.find((f) => f.id === id)?.base_url ?? null;
  }

  function choose(next: ServicePick) {
    pick = next;
    probe = null;
    listed = null;
    if (next.kind === "local" && found === null) scan();
    applyPick();
  }

  async function scan() {
    scanning = true;
    try { found = await scanLocalServices(); } catch { found = []; }
    scanning = false;
    const rows = localRows(found ?? [], services, role);
    if (pick.kind === "local" && !rows.find((r) => r.id === pickId(pick) && r.running)) {
      const first = rows.find((r) => r.running);
      pick = { kind: "local", id: first?.id ?? null };
    }
    applyPick();
  }

  /** The engine as the connection sees it, with the key typed now. */
  async function connectable(): Promise<ProviderConfig> {
    applyPick();
    const p = $state.snapshot(editing) as ProviderConfig;
    if (isLocal) p.api_key_ref = null;
    else if (apiKey.trim()) {
      // A new engine keeps the typed key under a reference of its own until it is saved.
      const ref = isNew ? pendingRef : (p.id || pendingRef);
      await setSecret(ref, apiKey);
      p.api_key_ref = ref;
      editing.api_key_ref = ref;
      keyPresent = true;
    }
    return p;
  }

  /** Asks the service for its models. `advance` moves to the model step when it answers. */
  async function connect(advance: boolean) {
    probe = { state: "wait", text: t("engines.connecting") };
    let r: ProbeResult;
    try {
      r = await probeProvider(await connectable());
    } catch (e) {
      probe = { state: "bad", text: errorMessage(e) };
      return;
    }
    probe = { state: r.ok ? "ok" : "bad", text: r.ok ? t("engines.connected") : describeFailure(r, t) };
    if (!r.ok) return;
    listed = r.models;
    for (const which of [role, other] as Role[]) {
      const s = sel[which];
      const stored = s.model;
      s.typed = r.models.length === 0 || (stored !== "" && !r.models.includes(stored));
      if (s.typed) s.typedModel = stored;
      else if (!stored) s.model = suggestedIn(r.models, preset?.suggested?.[which]) ?? r.models[0];
    }
    if (advance) step = "model";
  }

  /** Goes to the model step without a connection: the model is typed. */
  function skipConnect() {
    listed = [];
    sel[role].typed = true;
    sel[other].typed = true;
    step = "model";
  }

  const chosen = (r: Role): string => (hasList && !sel[r].typed ? sel[r].model : sel[r].typedModel.trim());
  const defaultLabel = (r: Role): string => [preset ? serviceName : "", chosen(r)].filter(Boolean).join(" ") || host || t("engines.external");

  /** The engine the form would store for the role. */
  function engineFor(r: Role): ProviderConfig {
    const p = structuredClone($state.snapshot(editing)) as ProviderConfig;
    if (isNew) p.capabilities = { ...(r === "asr" ? ASR_CAPS : LLM_CAPS) };
    p.model = chosen(r) || null;
    p.reasoning_effort = takesEffort(pick, services, r) ? sel[r].effort : null;
    p.label = sel[r].label.trim() || defaultLabel(r);
    if (isLocal) p.api_key_ref = null;
    return p;
  }

  // An edit that leaves the service, address, model and effort as they were needs no request.
  // svelte-ignore state_referenced_locally
  const needsCheck = $derived(isNew || !initial || roles.some((r) => checkKey(engineFor(r), r) !== checkKey(initial!, role)));
  const untested = $derived(needsCheck ? roles.filter((r) => !passed[checkKey(engineFor(r), r)]) : []);
  const canSave = $derived(keyKnown && roles.every((r) => !hasList || chosen(r) !== ""));

  function disarm() {
    clearTimeout(armTimer);
    armed = false;
  }

  async function testAndSave() {
    if (untested.length > 0) {
      if (!isLocal && !armed) {
        armed = true;
        armTimer = setTimeout(() => (armed = false), 4000);
        return;
      }
      disarm();
      busy = true;
      const results = await Promise.all(untested.map(async (r) => {
        const p = engineFor(r);
        let c: EngineCheck;
        try { c = await verifyProvider(p, r); }
        catch (e) { c = { ok: false, at: Math.floor(Date.now() / 1000), elapsed_ms: 0, message: errorMessage(e), role: r }; }
        return [r, p, c] as const;
      }));
      for (const [r, p, c] of results) {
        shown = { ...shown, [r]: c };
        if (c.ok) passed = { ...passed, [checkKey(p, r)]: c };
      }
      busy = false;
      if (results.some(([, , c]) => !c.ok)) return;
    }
    await save();
  }

  async function save() {
    busy = true;
    const ids = (config.value?.providers ?? []).map((x) => x.id);
    let saved = 0;
    try {
      for (const r of roles) {
        const p = engineFor(r);
        if (!p.id) {
          p.id = freshId(p.label, ids);
          ids.push(p.id);
        }
        if (!isLocal && apiKey.trim()) {
          p.api_key_ref = p.id;
          await setSecret(p.id, apiKey);
        } else if (!isLocal && !keyPresent) {
          p.api_key_ref = null;
        }
        config.value = await upsertProvider(p);
        const c = passed[checkKey(p, r)];
        if (c) await putEngineCheck(p.id, c).catch(() => {});
        saved++;
      }
      // Keys stored under an earlier or a pending reference are removed.
      if (editing.api_key_ref === pendingRef || (initial?.api_key_ref && initial.api_key_ref !== engineFor(role).api_key_ref && apiKey.trim())) {
        await deleteSecret(editing.api_key_ref ?? pendingRef).catch(() => {});
      }
      if (initial?.api_key_ref && isLocal) await deleteSecret(initial.api_key_ref).catch(() => {});
      savedAll = true;
      toast.ok(saved > 1 ? t("engines.added_n", { n: String(saved) }) : t("common.saved"));
      open = false;
    } catch (e) { toast.error(errorMessage(e)); }
    busy = false;
  }

  async function remove() {
    if (!initial) return;
    confirmDelete = false;
    const users = usedBy(config.value, initial.id);
    if (users.length > 0) {
      toast.error(t("engines.delete_in_use", { names: users.map((u) => u.name).join(t("home.list_sep")) }));
      return;
    }
    busy = true;
    try {
      await deleteProvider(initial.id);
      if (initial.api_key_ref) await deleteSecret(initial.api_key_ref);
      toast.ok(t("engines.deleted"));
      open = false;
    } catch (e) { toast.error(errorMessage(e)); }
    busy = false;
  }

  function cancel() {
    open = false;
  }

  // However the form goes, a key kept for an engine that was not added goes with it.
  onDestroy(() => {
    if (!savedAll && editing.api_key_ref === pendingRef) deleteSecret(pendingRef).catch(() => {});
  });

  const costText = $derived(isLocal
    ? t("engines.test_cost_local", { n: String(untested.length) })
    : t(isNew ? "engines.test_cost" : "engines.test_cost_save", { n: String(untested.length), name: serviceName }));
  const primaryText = $derived(untested.length === 0
    ? t(isNew ? "engines.add_done" : "common.save")
    : armed ? t(untested.length === 1 ? "engines.test_confirm_one" : "engines.test_confirm", { n: String(untested.length) })
    : t(isNew ? "engines.test_add" : "engines.test_save"));
  const time = (c: EngineCheck) => (c.elapsed_ms / 1000).toFixed(1);
</script>

{#snippet modelGroup(r: Role, heading: boolean)}
  {@const s = sel[r]}
  {@const list = (listed ?? []).filter((m) => !s.query.trim() || m.toLowerCase().includes(s.query.trim().toLowerCase()))}
  {@const hint = suggestedIn(listed ?? [], preset?.suggested?.[r])}
  {#if heading}<p class="group-t">{t(`engines.group.${r}`)}</p>{/if}
  <div class="fields">
    <span class="lab">{t("engines.model")}{#if hasList}<span class="req">{t("common.required")}</span>{/if}</span>
    <div>
      {#if hasList && listed}
        {#if listed.length > 8}
          <input type="search" class="search" bind:value={s.query} placeholder={t("engines.search_models")} aria-label={t("engines.search_models")} />
        {/if}
        <div class="models" role="radiogroup" aria-label={t("engines.model")}>
          {#each list as m (m)}
            <label class="model" class:on={!s.typed && s.model === m}>
              <input type="radio" name="model-{r}-{uid}" value={m} checked={!s.typed && s.model === m} onchange={() => { s.typed = false; s.model = m; disarm(); }} />
              <span class="mname">{m}</span>
              {#if m === hint}<small class="hint">{t("engines.suggested")}</small>{/if}
            </label>
          {:else}
            <p class="none">{t("engines.no_model_match")}</p>
          {/each}
        </div>
        <label class="model other" class:on={s.typed}>
          <input type="radio" name="model-{r}-{uid}" value="" checked={s.typed} onchange={() => { s.typed = true; disarm(); }} />
          <span class="mname">{t("engines.model_other")}</span>
          {#if s.typed}<input type="text" class="typed" bind:value={s.typedModel} aria-label={t("engines.model")} />{/if}
        </label>
      {:else}
        <input type="text" bind:value={s.typedModel} aria-label={t("engines.model")} oninput={disarm} />
        <small>{t("engines.model_type_help")}</small>
      {/if}
    </div>
    <label for="f-label-{r}-{uid}">{t("engines.label")}</label>
    <div>
      <input id="f-label-{r}-{uid}" type="text" bind:value={s.label} placeholder={defaultLabel(r)} />
    </div>
    {#if takesEffort(pick, services, r)}
      <label for="f-effort-{r}-{uid}">{t("engines.effort")}</label>
      <div>
        <select id="f-effort-{r}-{uid}" value={s.effort ?? ""} onchange={(e) => { s.effort = (e.currentTarget as HTMLSelectElement).value || null; disarm(); }}>
          {#each EFFORTS as v (v ?? "default")}<option value={v ?? ""}>{t(`engines.effort.${v ?? "default"}`)}</option>{/each}
        </select>
        <small>{t("engines.effort_help")}</small>
      </div>
    {/if}
    {#if shown[r]}
      {@const c = shown[r]!}
      <span></span>
      <div class="result" role="status">
        {#if c.ok}
          <ServiceAnswer tone="ok" text={t("engines.check_ok_in", { time: time(c) })} />
        {:else}
          <ServiceAnswer tone="bad" text={c.status ? t("engines.check_failed_status", { status: String(c.status) }) : t("engines.check_failed")} body={c.message} detail={c.detail} />
        {/if}
      </div>
    {/if}
  </div>
{/snippet}

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
        {@const clouds = cloudFor(services, role)}
        {#if clouds.length > 0}
          <p class="group-t">{t("engines.service_cloud")}</p>
          <div class="svcs">
            {#each clouds as s (s.id)}
              <button type="button" class="svc" class:on={pick.kind === "cloud" && pickId(pick) === s.id} onclick={() => choose({ kind: "cloud", id: s.id })}>
                <b>{s.label}</b><small>{t(s.key_required ? "engines.service_key_sub" : "engines.service_no_key_sub")}</small>
              </button>
            {/each}
          </div>
        {/if}
        <div class="svcs two">
          {#if localFor(services, role).length > 0}
            <button type="button" class="svc" class:on={pick.kind === "local"} onclick={() => choose({ kind: "local", id: null })}>
              <b>{t("engines.service_local")}</b><small>{t("engines.service_local_sub")}</small>
            </button>
          {/if}
          <button type="button" class="svc" class:on={pick.kind === "custom"} onclick={() => choose({ kind: "custom" })}>
            <b>{t("engines.service_custom")}</b><small>{t("engines.service_custom_sub")}</small>
          </button>
        </div>
        <div class="divider"></div>
        {#key pick.kind === "local" ? "local" : pick.kind === "custom" ? "custom" : "cloud"}
          <Fold>
            {#if pick.kind === "cloud" && preset}
              <div class="fields">
                <span class="lab">{t("engines.base_url")}</span>
                <div class="fact"><code>{host}</code></div>
                <label for="f-key-{uid}">{t("engines.api_key")}{#if preset.key_required}<span class="req">{t("common.required")}</span>{/if}</label>
                <div>
                  <input id="f-key-{uid}" type="password" bind:value={apiKey} placeholder={keyPresent ? t("engines.key_present") : ""} autocomplete="off" />
                  <small>{t("engines.key_stored")}{#if preset.key_url}<button type="button" class="link" onclick={() => openServiceKeyPage(preset.id)}>{t("engines.key_get", { name: preset.label })}</button>{/if}</small>
                </div>
              </div>
            {:else if pick.kind === "local"}
              {@const rows = localRows(found ?? [], services, role)}
              <p class="status">
                {#if scanning || found === null}{t("engines.local_scanning")}{:else}{t("engines.local_found", { n: String(rows.filter((r) => r.running).length) })}{/if}
                <button type="button" class="btn quiet small" onclick={scan} disabled={scanning}>{t("engines.local_rescan")}</button>
              </p>
              {#if found !== null}
                <div class="models" role="radiogroup" aria-label={t("engines.service_local")}>
                  {#each rows as f (f.id)}
                    <label class="model" class:on={pickId(pick) === f.id} class:off={!f.running}>
                      <input type="radio" name="local-{uid}" disabled={!f.running} checked={pickId(pick) === f.id} onchange={() => { pick = { kind: "local", id: f.id }; applyPick(); probe = null; listed = null; }} />
                      <span class="mname">{f.label}</span>
                      <small class="hint">{f.running ? new URL(f.base_url).host : t("engines.local_not_running")}</small>
                    </label>
                  {/each}
                </div>
              {/if}
            {:else}
              <div class="fields">
                <label for="f-url-{uid}">{t("engines.base_url")}<span class="req">{t("common.required")}</span></label>
                <div>
                  <input id="f-url-{uid}" type="url" bind:value={customUrl} oninput={applyPick} placeholder="http://localhost:11434/v1" />
                  <small>{t("engines.base_url_help")}</small>
                </div>
                <label for="f-key-{uid}">{t("engines.api_key")}</label>
                <div>
                  <input id="f-key-{uid}" type="password" bind:value={apiKey} placeholder={keyPresent ? t("engines.key_present") : ""} autocomplete="off" />
                  <small>{t("engines.api_key_help")}</small>
                </div>
              </div>
            {/if}
          </Fold>
        {/key}
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
        {@render modelGroup(role, isNew && servesOther(pick, services, role))}
        {#if isNew && servesOther(pick, services, role)}
          <div class="also">
            <Switch bind:checked={alsoOn} label={t(`engines.also_add.${other}`)} onchange={disarm} />
            <span><b>{t(`engines.also_add.${other}`)}</b><small>{t("engines.also_add_sub")}</small></span>
          </div>
          {#if alsoOn}
            <Fold>{@render modelGroup(other, true)}</Fold>
          {/if}
        {/if}
        {#if untested.length > 0}
          <p class="cost"><Icon name="info" size={14} /><span>{costText}</span></p>
        {/if}
      {/if}
    </div>
  {/if}

  {#snippet actions()}
    {#if confirmDiscard}
      <button type="button" class="btn" bind:this={keepButton} onclick={() => (confirmDiscard = false)}>{t("common.cancel")}</button>
      <button type="button" class="btn danger" onclick={cancel}>{t("common.discard")}</button>
    {:else if confirmDelete}
      <button type="button" class="btn" bind:this={keepButton} onclick={() => (confirmDelete = false)}>{t("common.cancel")}</button>
      <button type="button" class="btn danger" onclick={remove} disabled={busy}>{t("engines.delete")}</button>
    {:else if step === "connect"}
      {#if probe?.state === "bad"}<button type="button" class="btn quiet lead" onclick={skipConnect}>{t("engines.continue_anyway")}</button>{/if}
      <button type="button" class="btn" onclick={cancel}>{t("common.cancel")}</button>
      <button type="button" class="btn primary" onclick={() => connect(true)} disabled={!canConnect || probe?.state === "wait"}>{t("engines.connect")}</button>
    {:else}
      {#if !isNew}<button type="button" class="btn quiet danger lead" onclick={askDelete} disabled={busy}>{t("engines.delete")}</button>{/if}
      <button type="button" class="btn quiet" class:lead={isNew} onclick={() => { disarm(); step = "connect"; }} disabled={busy}><Icon name="chevron-left" size={14} />{t(isNew ? "engines.back" : "engines.change_connection")}</button>
      <button type="button" class="btn" onclick={cancel}>{t("common.cancel")}</button>
      <button type="button" class="btn primary" class:ask={armed} onclick={testAndSave} disabled={busy || !canSave}>{busy && untested.length > 0 ? t("engines.testing") : primaryText}</button>
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
  .group-t { margin: 0; font-size: var(--text-xs); font-weight: 600; color: var(--ink-3); }
  .svcs { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; }
  .svcs.two { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .svc { display: flex; flex-direction: column; gap: 2px; min-width: 0; padding: 9px 12px; border: 1px solid var(--line-strong); border-radius: 12px; background: var(--surface); color: var(--ink); font: inherit; text-align: left; cursor: default; }
  .svc:hover { background: var(--sunken); }
  .svc.on { border-color: var(--accent); background: var(--accent-soft); }
  .svc b { font-size: var(--text-control); font-weight: 600; }
  .svc small { color: var(--ink-2); font-size: var(--text-xs); line-height: 1.4; }
  .divider { height: 1px; background: var(--line); }
  .fields { display: grid; grid-template-columns: minmax(96px, max-content) minmax(0, 1fr); gap: 14px; align-items: start; }
  .fields > label, .fields > .lab { color: var(--ink-2); padding-top: 7px; white-space: nowrap; }
  .fields input[type="url"], .fields input[type="password"], .fields input[type="text"], .fields .search, .fields select { width: 100%; }
  .fields small { display: block; margin: 5px 2px 0; color: var(--ink-2); font-size: var(--text-xs); line-height: 1.45; }
  .fact { padding-top: 7px; }
  .fact code { font-size: var(--text-xs); color: var(--ink-2); background: var(--sunken); padding: 2px 6px; border-radius: 6px; }
  .link { margin-left: 6px; padding: 0; border: 0; background: none; color: var(--accent); font: inherit; cursor: default; }
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
  .model.off { color: var(--ink-3); }
  .model.off:hover { background: none; }
  .model input[type="radio"] { margin: 0; }
  .mname { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hint { margin-left: auto; flex: none; color: var(--ink-3); font-size: var(--text-xs); }
  .model .typed { flex: 1; min-width: 0; height: 30px; }
  .none { margin: 0; padding: 12px; color: var(--ink-2); font-size: var(--text-sm); }
  .also { display: flex; gap: 12px; align-items: flex-start; padding-top: 14px; border-top: 1px solid var(--line); }
  .also b { display: block; font-size: var(--text-control); font-weight: 600; }
  .also small { color: var(--ink-2); font-size: var(--text-xs); }
  .result { margin: -6px 0 0; font-size: var(--text-sm); line-height: 1.45; overflow-wrap: anywhere; }
  .cost { display: flex; gap: 9px; align-items: flex-start; margin: 0; padding: 10px 12px; border-radius: 10px; background: var(--sunken); color: var(--ink-2); font-size: var(--text-sm); line-height: 1.5; }
  .cost :global(svg) { flex: none; margin-top: 3px; }
  .btn.ask { background: var(--live); border-color: var(--live); color: #fff; }
  .lead { margin-right: auto; margin-left: -8px; }
</style>
