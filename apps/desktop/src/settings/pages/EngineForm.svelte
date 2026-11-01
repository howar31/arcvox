<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { deleteProvider, deleteSecret, errorMessage, hasSecret, listServices, openServiceKeyPage, openServiceTermsPage, probeProvider, putEngineCheck, scanLocalServices, setSecret, upsertProvider, verifyProvider, type EngineCheck, type LocalService, type ProbeResult, type ProviderConfig, type ServicePreset, profilesPresets, upsertProfile, type PresetView, type Profile } from "../../lib/api";
  import { blankHttp, freshId, type Role } from "../../lib/engines";
  import { t } from "../../lib/i18n";
  import { ASR_CAPS, describeFailure, LLM_CAPS } from "../../lib/providerForm";
  import { checkBody, checkKey, cloudFor, EFFORTS, localFor, localRows, modesOf, offersWordTimes, otherRole, pickId, pickOf, servesOther, suggestedIn, takesEffort, type ServicePick } from "../../lib/serviceForm";
  import { config } from "../../lib/store.svelte";
  import { freshName, namesEngine, templateEngines, withoutEngine } from "../../lib/profileEdit";
  import { engineDefaultName } from "../../lib/stages";
  import Choices from "../../lib/ui/Choices.svelte";
  import Fold from "../../lib/ui/Fold.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import ServiceAnswer from "../../lib/ui/ServiceAnswer.svelte";
  import Select from "../../lib/ui/Select.svelte";
  import Steps from "../../lib/ui/Steps.svelte";
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

  type Sel = { model: string; typed: boolean; typedModel: string; label: string; effort: string | null; mode: string | null; words: boolean };
  const blankSel = (): Sel => ({ model: "", typed: false, typedModel: "", label: "", effort: null, mode: null, words: false });
  // The last choice of the model list: the name is typed. No listed model id has this form.
  const OTHER_MODEL = "\u0000other";
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
    sel[role].mode = initial.transcribe_mode ?? null;
    sel[role].words = !!initial.word_times;
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
  let presets = $state<PresetView[]>([]);
  // Where the stages on a deleted engine go.
  let moveTo = $state<"default" | "template">("default");
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
    if (busy) return false;
    // Esc on a question answers no; the form stays as it was.
    if (confirmDiscard || confirmDelete) { confirmDiscard = confirmDelete = false; return false; }
    if (armed) { disarm(); return false; }
    if (!dirty) return true;
    confirmDiscard = true;
    tick().then(() => keepButton?.focus());
    return false;
  }

  const engineId = $derived(initial?.id ?? "");
  const naming = $derived((config.value?.profiles ?? []).filter((p) => namesEngine(p, engineId)));
  const isDefaultEngine = $derived((["asr", "llm"] as Role[]).some((r) => config.value?.default_engines[r] === engineId));
  const templateDiffers = $derived(naming.some((p) => {
    const to = templateEngines(p, engineId, presets);
    return (p.asr_provider === engineId && to.asr) || (p.llm_provider === engineId && to.llm);
  }));
  const defaultName = $derived.by(() => {
    const id = config.value?.default_engines[role];
    return config.value?.providers.find((p) => p.id === id)?.label ?? id ?? "";
  });

  async function askDelete() {
    moveTo = "default";
    presets = await profilesPresets().catch(() => []);
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
  // The first mode is the service's default, which is not sent.
  const modeFor = (r: Role): string | null => (r === "asr" && modesOf(pick, services, chosen(r)).slice(1).includes(sel[r].mode ?? "") ? sel[r].mode : null);
  // Word times exist in the service's default mode.
  const wordsOffered = (r: Role): boolean => r === "asr" && !modeFor(r) && offersWordTimes(pick, services, chosen(r));
  const wordsFor = (r: Role): boolean => wordsOffered(r) && sel[r].words;
  const effortFor = (r: Role): string | null => (takesEffort(pick, services, r, chosen(r)) ? sel[r].effort : null);
  /** The engine's name when none is typed: engines of one model differ by what they set. */
  const defaultLabel = (r: Role): string => engineDefaultName(serviceName, chosen(r), { transcribe_mode: modeFor(r), reasoning_effort: effortFor(r), word_times: wordsFor(r) });
  // A stored name that is the default one was never typed, so it goes on following the settings.
  let labelChecked = false;
  $effect(() => {
    if (labelChecked || !initial || !services.length) return;
    labelChecked = true;
    const typed = sel[role].label, auto = defaultLabel(role);
    if (typed === auto || (typed.startsWith(`${auto}_`) && /^\d+$/.test(typed.slice(auto.length + 1)))) sel[role].label = "";
  });

  /** The engine the form would store for the role. */
  function engineFor(r: Role): ProviderConfig {
    const p = structuredClone($state.snapshot(editing)) as ProviderConfig;
    if (isNew) p.capabilities = { ...(r === "asr" ? ASR_CAPS : LLM_CAPS) };
    p.model = chosen(r) || null;
    p.reasoning_effort = effortFor(r);
    p.transcribe_mode = modeFor(r);
    p.word_times = wordsFor(r);
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
        // A name left to the form differs from the others of its kind; a typed one stays as typed.
        if (!sel[r].label.trim()) {
          const kin = (config.value?.providers ?? []).filter((x) => x.id !== initial?.id && (r === "asr" ? x.capabilities.transcribe : x.capabilities.chat));
          p.label = freshName(p.label, kin.map((x) => x.label), "_");
        }
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
    if (isDefaultEngine) return;
    confirmDelete = false;
    busy = true;
    try {
      for (const p of $state.snapshot(naming) as Profile[]) {
        const to = moveTo === "template" ? templateEngines(p, initial.id, presets) : { asr: null, llm: null };
        config.value = await upsertProfile(withoutEngine(p, initial.id, to));
      }
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
  {@const hint = suggestedIn(listed ?? [], preset?.suggested?.[r])}
  {@const modes = r === "asr" ? modesOf(pick, services, chosen(r)) : []}
  {#if heading}<p class="group-t">{t(`engines.group.${r}`)}</p>{/if}
  <div class="fields">
    <span class="lab">{t("engines.model")}{#if hasList}<span class="req">{t("common.required")}</span>{/if}</span>
    <div>
      {#if hasList && listed}
        <Select
          variable width="100%" label={t("engines.model")} value={s.typed ? OTHER_MODEL : s.model}
          onchange={(v) => { s.typed = v === OTHER_MODEL; if (!s.typed) s.model = v; disarm(); }}
          options={[...(listed ?? []).map((m) => ({ value: m, label: m === hint ? `${m}（${t("engines.suggested")}）` : m })), { value: OTHER_MODEL, label: t("engines.model_other") }]}
        />
        {#if s.typed}<Fold><input type="text" class="typed" bind:value={s.typedModel} aria-label={t("engines.model")} oninput={disarm} /></Fold>{/if}
      {:else}
        <input type="text" bind:value={s.typedModel} aria-label={t("engines.model")} oninput={disarm} />
        <small>{t("engines.model_type_help")}</small>
      {/if}
    </div>
    <label for="f-label-{r}-{uid}">{t("engines.label")}</label>
    <div>
      <input id="f-label-{r}-{uid}" type="text" bind:value={s.label} placeholder={defaultLabel(r)} />
    </div>
    {#if modes.length}
      <span class="lab">{t("engines.transcribe_mode")}</span>
      <div>
        <Choices
          label={t("engines.transcribe_mode")}
          bind:value={() => s.mode ?? modes[0], (v) => { s.mode = v; disarm(); }}
          options={modes.map((m) => ({ value: m, label: t(`engines.transcribe_mode.${m}`), help: t(`engines.transcribe_mode.${m}_help`), info: m === modes[0] ? undefined : t(`engines.transcribe_mode.${m}_info`, { name: serviceName }) }))}
        />
        {#if modeFor(r) === "smart"}<Fold><small class="mode-note">{t("engines.transcribe_mode.smart_note")}</small></Fold>{/if}
        {#if wordsOffered(r)}<Fold>
          <div class="words">
            <div class="words-row">
              <span>{t("engines.word_times")}</span>
              <Switch bind:checked={s.words} label={t("engines.word_times")} onchange={disarm} />
            </div>
            <small>{t(s.words ? "engines.word_times_on" : "engines.word_times_off")}</small>
            {#if s.words}<Fold><p class="warn"><Icon name="warning" size={14} /><span>{t("engines.word_times_warning")}</span></p></Fold>{/if}
          </div>
        </Fold>{/if}
      </div>
    {/if}
    {#if takesEffort(pick, services, r, chosen(r))}
      <span class="lab">{t("engines.effort")}</span>
      <div>
        <Select
          width="100%" label={t("engines.effort")} value={s.effort ?? ""} onchange={(v) => { s.effort = v || null; disarm(); }}
          options={EFFORTS.map((v) => ({ value: v ?? "", label: t(`engines.effort.${v ?? "default"}`) }))}
        />
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
          <ServiceAnswer tone="bad" text={c.status ? t("engines.check_failed_status", { status: String(c.status) }) : t("engines.check_failed")} body={checkBody(c)} detail={c.detail} />
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
    {#if isDefaultEngine}
      <p>{t("engines.delete_is_default", { kind: t(`engines.group.${role}`) })}</p>
    {:else if naming.length}
      <p>{t("engines.delete_moves", { names: naming.map((p) => p.name).join(t("home.list_sep")) })}</p>
      {#if templateDiffers}
        <Choices
          label={t("engines.delete_title")}
          bind:value={moveTo}
          options={[
            { value: "default" as const, label: t("engines.delete_to_default"), help: t("engines.delete_to_default_help", { kind: t(`engines.group.${role}`), name: defaultName }) },
            { value: "template" as const, label: t("engines.delete_to_template"), help: t("engines.delete_to_template_help") },
          ]}
        />
      {:else}
        <p>{t("engines.delete_to_default_help", { kind: t(`engines.group.${role}`), name: defaultName })}</p>
      {/if}
    {/if}
  {:else}
    <div class="form">
      <Steps current={step} steps={[{ id: "connect", label: t("engines.step_connect") }, { id: "model", label: t("engines.step_model"), disabled: listed === null }]} onpick={(id) => (step = id as typeof step)} />
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
        {#if pick.kind === "cloud" && preset}
          <p class="cost"><Icon name="info" size={14} /><span>{t("engines.data_note", { name: preset.label })}{#if preset.terms_url} <button type="button" class="link" onclick={() => openServiceTermsPage(preset.id)}>{t("engines.data_terms")}</button>{/if}</span></p>
        {/if}
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
      <button type="button" class="btn danger" onclick={remove} disabled={busy || isDefaultEngine}>{t("engines.delete")}</button>
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
  .fields input[type="url"], .fields input[type="password"], .fields input[type="text"] { width: 100%; }
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
  .models { border: 1px solid var(--line); border-radius: 12px; overflow: hidden; max-height: 220px; overflow-y: auto; }
  .model { display: flex; align-items: center; gap: 10px; padding: 8px 12px; border-top: 1px solid var(--line); font-size: var(--text-control); cursor: default; }
  .model:first-child { border-top: 0; }
  .model:hover { background: var(--sunken); }
  .model.on { background: var(--accent-soft); }
  .model.off { color: var(--ink-3); }
  .model.off:hover { background: none; }
  .model input[type="radio"] { margin: 0; }
  .mname { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hint { margin-left: auto; flex: none; color: var(--ink-3); font-size: var(--text-xs); }
  .typed { margin-top: 6px; }
  .none { margin: 0; padding: 12px; color: var(--ink-2); font-size: var(--text-sm); }
  .also { display: flex; gap: 12px; align-items: flex-start; padding-top: 14px; border-top: 1px solid var(--line); }
  .also b { display: block; font-size: var(--text-control); font-weight: 600; }
  .also small { color: var(--ink-2); font-size: var(--text-xs); }
  .result { margin: -6px 0 0; font-size: var(--text-sm); line-height: 1.45; overflow-wrap: anywhere; }
  .mode-note { display: block; margin-top: 8px; }
  .words { margin-top: 12px; }
  .words-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .warn { display: flex; gap: 9px; align-items: flex-start; margin: 8px 0 0; color: var(--warn); font-size: var(--text-sm); line-height: 1.55; }
  .warn :global(svg) { flex: none; margin-top: 3px; }
  .cost { display: flex; gap: 9px; align-items: flex-start; margin: 0; padding: 10px 12px; border-radius: 10px; background: var(--sunken); color: var(--ink-2); font-size: var(--text-sm); line-height: 1.5; }
  .cost :global(svg) { flex: none; margin-top: 3px; }
  .btn.ask { background: var(--live); border-color: var(--live); color: #fff; }
  .lead { margin-right: auto; margin-left: -8px; }
</style>
