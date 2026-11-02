<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { deleteSecret, errorMessage, hasSecret, listServices, openServiceKeyPage, openServiceTermsPage, probeProvider, putEngineCheck, scanLocalServices, setSecret, upsertProvider, verifyProvider, type EngineCheck, type LocalService, type ProbeResult, type ProviderConfig, type ServicePreset, profilesPresets, type PresetView } from "../../lib/api";
  import { blankHttp, freshId, type Role } from "../../lib/engines";
  import { t } from "../../lib/i18n";
  import { ASR_CAPS, describeFailure, LLM_CAPS } from "../../lib/providerForm";
  import { checkBody, checkKey, cloudFor, EFFORTS, localFor, localRows, modesOf, offersWordTimes, otherRole, pickId, pickOf, servesOther, suggestedIn, takesEffort, type ServicePick } from "../../lib/serviceForm";
  import { config } from "../../lib/store.svelte";
  import { freshName } from "../../lib/profileEdit";
  import { removeEngines, type MoveTo } from "../../lib/engineRemoval";
  import RemovalNote from "./engines/RemovalNote.svelte";
  import { engineDefaultName } from "../../lib/stages";
  import { tip } from "../../lib/tip";
  import { wantsSearch } from "../../lib/dropdown";
  import Choices from "../../lib/ui/Choices.svelte";
  import Dropdown from "../../lib/ui/Dropdown.svelte";
  import InfoTip from "../../lib/ui/InfoTip.svelte";
  import MenuOption from "../../lib/ui/MenuOption.svelte";
  import Segmented from "../../lib/ui/Segmented.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
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
  let editing = $state<ProviderConfig>(initial ? structuredClone($state.snapshot(initial)) : blankHttp(role));
  let services = $state<ServicePreset[]>([]);
  let loaded = $state(false);
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
  /** One engine the form would store: editing holds the one engine, adding a list of them picked from the models. */
  type Entry = { key: string; role: Role; sel: Sel; open: boolean };
  let entries = $state<Entry[]>([]);
  // svelte-ignore state_referenced_locally
  if (initial) {
    entries = [{ key: "edit", role, open: true, sel: { ...blankSel(), model: initial.model ?? "", typedModel: initial.model ?? "", label: initial.label, effort: initial.reasoning_effort ?? null, mode: initial.transcribe_mode ?? null, words: !!initial.word_times } }];
  }
  // svelte-ignore state_referenced_locally
  let addRole = $state<Role>(role);
  let modelQuery = $state("");
  // Real requests that passed, by what they depended on; one is not sent again.
  let passed = $state<Record<string, EngineCheck>>({});
  // The last result shown on each entry, by its key.
  let shown = $state<Record<string, EngineCheck>>({});
  // A run of real requests going one entry after another.
  let run = $state<{ total: number; done: number; passed: number; failed: number; at: string | null; queue: string[] } | null>(null);
  let stopping = false;
  let armed = $state(false);
  let armTimer: ReturnType<typeof setTimeout> | undefined;
  let busy = $state(false);
  let confirmDelete = $state(false);
  let presets = $state<PresetView[]>([]);
  // Where the stages on a deleted engine go.
  let moveTo = $state<MoveTo>("default");
  /**
   * What the user can change, as text: the service, its address and key, and each engine with its settings
   * as they would be stored. Moving between the steps or connecting changes none of it.
   */
  function formState(): string {
    return JSON.stringify({
      pick, customUrl: customUrl.trim(), key: apiKey.trim(),
      entries: entries.map((e) => [e.role, chosen(e), e.sel.label.trim() || defaultLabel(e), modeFor(e), wordsFor(e), effortFor(e)]),
    });
  }
  // Taken once the form has set itself up, so its own first settings are not changes.
  let pristine = $state<string | null>(null);
  const dirty = $derived(pristine !== null && formState() !== pristine);

  const preset = $derived(pickId(pick) === null ? undefined : services.find((s) => s.id === pickId(pick)));
  const isLocal = $derived(pick.kind === "local");
  const keyNeeded = $derived(pick.kind === "cloud" && !!preset?.key_required);
  const hasList = $derived(!!listed && listed.length > 0);
  const both = $derived(isNew && servesOther(pick, services, role));
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
    disarm();
    return true;
  }

  const engineId = $derived(initial?.id ?? "");
  const isDefaultEngine = $derived((["asr", "llm"] as Role[]).some((r) => config.value?.default_engines[r] === engineId));

  async function askDelete() {
    moveTo = "default";
    presets = await profilesPresets().catch(() => []);
    confirmDelete = true;
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
    // The form shows once the service is picked: the first pick then appears without sliding in.
    loaded = true;
    if (pick.kind === "local") await scan();
    pristine = formState();
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
    // The engine being edited keeps its model; one the service no longer lists is typed.
    for (const e of entries.filter((x) => x.key === "edit")) {
      const stored = e.sel.model;
      e.sel.typed = r.models.length === 0 || (stored !== "" && !r.models.includes(stored));
      if (e.sel.typed) e.sel.typedModel = stored;
      else if (!stored) e.sel.model = suggestedIn(r.models, preset?.suggested?.[e.role]) ?? r.models[0];
    }
    if (advance) step = "model";
  }

  /** Goes to the model step without a connection: the model is typed. */
  function skipConnect() {
    listed = [];
    for (const e of entries) e.sel.typed = true;
    step = "model";
  }

  const chosen = (e: Entry): string => (hasList && !e.sel.typed ? e.sel.model : e.sel.typedModel.trim());
  // The first mode is the service's default, which is not sent.
  const modeFor = (e: Entry): string | null => (e.role === "asr" && modesOf(pick, services, chosen(e)).slice(1).includes(e.sel.mode ?? "") ? e.sel.mode : null);
  // Word times exist in the service's default mode.
  const wordsOffered = (e: Entry): boolean => e.role === "asr" && !modeFor(e) && offersWordTimes(pick, services, chosen(e));
  const wordsFor = (e: Entry): boolean => wordsOffered(e) && e.sel.words;
  const effortFor = (e: Entry): string | null => (takesEffort(pick, services, e.role, chosen(e)) ? e.sel.effort : null);
  /** The engine's name when none is typed: engines of one model differ by what they set. */
  const defaultLabel = (e: Entry): string => engineDefaultName(serviceName, chosen(e), { transcribe_mode: modeFor(e), reasoning_effort: effortFor(e), word_times: wordsFor(e) });
  // A stored name that is the default one was never typed, so it goes on following the settings.
  let labelChecked = false;
  $effect(() => {
    if (labelChecked || !initial || !services.length || !entries[0]) return;
    labelChecked = true;
    const e = entries[0];
    const typed = e.sel.label, auto = defaultLabel(e);
    if (typed === auto || (typed.startsWith(`${auto}_`) && /^\d+$/.test(typed.slice(auto.length + 1)))) e.sel.label = "";
  });

  /** The engine the form would store for the entry. */
  function engineFor(e: Entry): ProviderConfig {
    const p = structuredClone($state.snapshot(editing)) as ProviderConfig;
    if (isNew) p.capabilities = { ...(e.role === "asr" ? ASR_CAPS : LLM_CAPS) };
    p.model = chosen(e) || null;
    p.reasoning_effort = effortFor(e);
    p.transcribe_mode = modeFor(e);
    p.word_times = wordsFor(e);
    p.label = e.sel.label.trim() || defaultLabel(e);
    if (isLocal) p.api_key_ref = null;
    return p;
  }
  const keyOf = (e: Entry): string => checkKey(engineFor(e), e.role);
  const passedOn = (e: Entry): boolean => !!passed[keyOf(e)];

  // An edit that leaves the service, address, model and effort as they were needs no request.
  // svelte-ignore state_referenced_locally
  const needsCheck = $derived(isNew || !initial || entries.some((e) => keyOf(e) !== checkKey(initial!, role)));
  const untested = $derived(needsCheck ? entries.filter((e) => !passedOn(e)) : []);
  const ready = $derived(entries.filter(passedOn));
  const failedCount = $derived(entries.filter((e) => !passedOn(e) && shown[e.key] && !shown[e.key].ok).length);
  const canSave = $derived(keyKnown && entries.length > 0 && entries.every((e) => !hasList || chosen(e) !== ""));

  /** Adds an engine of the model to the list, open for its settings; the others close. */
  function addEntry(model: string | null) {
    for (const e of entries) e.open = false;
    const r: Role = both ? addRole : role;
    const key = Math.random().toString(36).slice(2);
    entries.push({ key, role: r, open: true, sel: { ...blankSel(), model: model ?? "", typed: model === null || !hasList } });
    disarm();
    // The new entry is brought into view once the others have folded shut.
    setTimeout(() => {
      const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
      document.querySelector(`[data-entry="${key}"]`)?.scrollIntoView({ block: "nearest", behavior: still ? "auto" : "smooth" });
    }, 240);
  }
  // Removing every entry is asked once more: the first press turns the link into the question for 4 seconds.
  let clearArmed = $state(false);
  let clearTimer: ReturnType<typeof setTimeout> | undefined;
  function clearAll() {
    if (!clearArmed) {
      clearArmed = true;
      clearTimeout(clearTimer);
      clearTimer = setTimeout(() => (clearArmed = false), 4000);
      return;
    }
    clearTimeout(clearTimer);
    clearArmed = false;
    entries = [];
    disarm();
  }
  function dropEntry(e: Entry) {
    entries = entries.filter((x) => x.key !== e.key);
    disarm();
  }
  /** The name the entry gets when it is added: a name left to the form takes `_2`, `_3` among the engines of its role and the entries before it. */
  function shownName(e: Entry): string {
    const typed = e.sel.label.trim();
    if (typed) return typed;
    const kin = (config.value?.providers ?? []).filter((x) => x.id !== initial?.id && (e.role === "asr" ? x.capabilities.transcribe : x.capabilities.chat)).map((x) => x.label);
    for (const x of entries) {
      if (x.key === e.key) break;
      if (x.role === e.role) kin.push(shownName(x));
    }
    return freshName(defaultLabel(e), kin, "_");
  }
  /** How many entries of this form use the model. */
  const addedOf = (m: string): number => entries.filter((e) => !e.sel.typed && e.sel.model === m).length;
  /** What the catalogue knows of a model: its modes and word times, or that it takes an effort. */
  function modelFacts(m: string): string {
    const r: Role = both ? addRole : role;
    const modes = r === "asr" ? modesOf(pick, services, m) : [];
    if (modes.length) {
      const list = modes.map((x) => t(`engines.transcribe_mode.${x}`)).join(t("home.list_sep"));
      return offersWordTimes(pick, services, m) ? t("engines.model_facts_modes_words", { modes: list, first: t(`engines.transcribe_mode.${modes[0]}`) }) : t("engines.model_facts_modes", { modes: list });
    }
    return takesEffort(pick, services, r, m) ? t("engines.model_facts_effort") : "";
  }
  const shownModels = $derived((listed ?? []).filter((m) => m.toLowerCase().includes(modelQuery.trim().toLowerCase())));
  /** The entry's settings in one line, under its name. */
  function summary(e: Entry): string {
    const parts: string[] = [];
    const modes = e.role === "asr" ? modesOf(pick, services, chosen(e)) : [];
    if (modes.length) parts.push(t(`engines.transcribe_mode.${modeFor(e) ?? modes[0]}`));
    if (wordsFor(e)) parts.push(t("engines.word_times"));
    if (takesEffort(pick, services, e.role, chosen(e))) parts.push(`${t("engines.effort")}：${t(`engines.effort.${effortFor(e) ?? "default"}`)}`);
    if (e.sel.typed) parts.unshift(chosen(e) || t("engines.model_none"));
    return parts.join(t("home.list_sep"));
  }
  function statusOf(e: Entry): "run" | "wait" | "ok" | "bad" | null {
    if (run?.at === e.key) return "run";
    if (run?.queue.includes(e.key)) return "wait";
    if (passedOn(e)) return shown[e.key] ? "ok" : null;
    if (shown[e.key] && !shown[e.key].ok) return "bad";
    return null;
  }

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
      stopping = false;
      const list = [...untested];
      run = { total: list.length, done: 0, passed: 0, failed: 0, at: null, queue: list.map((e) => e.key) };
      for (const e of list) {
        if (stopping) break;
        run = { ...run!, at: e.key, queue: run!.queue.filter((k) => k !== e.key) };
        const p = engineFor(e);
        let c: EngineCheck;
        try { c = await verifyProvider(p, e.role); }
        catch (err) { c = { ok: false, at: Math.floor(Date.now() / 1000), elapsed_ms: 0, message: errorMessage(err), role: e.role }; }
        shown = { ...shown, [e.key]: c };
        if (c.ok) passed = { ...passed, [checkKey(p, e.role)]: c };
        else e.open = true;
        run = { ...run!, done: run!.done + 1, passed: run!.passed + (c.ok ? 1 : 0), failed: run!.failed + (c.ok ? 0 : 1) };
      }
      run = null;
      busy = false;
      if (entries.some((e) => !passedOn(e))) return;
    }
    await save(entries);
  }

  async function save(list: Entry[]) {
    busy = true;
    const ids = (config.value?.providers ?? []).map((x) => x.id);
    let saved = 0;
    try {
      for (const e of list) {
        const p = engineFor(e);
        // A name left to the form differs from the others of its kind; a typed one stays as typed.
        if (!e.sel.label.trim()) {
          const kin = (config.value?.providers ?? []).filter((x) => x.id !== initial?.id && (e.role === "asr" ? x.capabilities.transcribe : x.capabilities.chat));
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
        const c = passed[checkKey(p, e.role)];
        if (c) await putEngineCheck(p.id, c).catch(() => {});
        saved++;
      }
      // Keys stored under an earlier or a pending reference are removed.
      if (editing.api_key_ref === pendingRef || (initial?.api_key_ref && entries[0] && initial.api_key_ref !== engineFor(entries[0]).api_key_ref && apiKey.trim())) {
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
    busy = true;
    try {
      await removeEngines([$state.snapshot(initial) as ProviderConfig], moveTo, presets);
      confirmDelete = false;
      toast.ok(t("engines.deleted"));
      open = false;
    } catch (e) { toast.error(errorMessage(e)); }
    busy = false;
  }

  let dialog: { cancel: () => void } | undefined = $state();
  /** The cancel button: the same as Esc, so changes are asked about first. */
  function cancel() {
    dialog?.cancel();
  }

  // However the form goes, a key kept for an engine that was not added goes with it.
  onDestroy(() => {
    if (!savedAll && editing.api_key_ref === pendingRef) deleteSecret(pendingRef).catch(() => {});
  });

  const costText = $derived(isLocal
    ? t("engines.test_cost_local", { n: String(untested.length) })
    : t(isNew ? "engines.test_cost" : "engines.test_cost_save", { n: String(untested.length), name: serviceName }));
  const primaryText = $derived(isNew && entries.length === 0 ? t("engines.test_add") : untested.length === 0
    ? (isNew ? t("engines.add_n", { n: String(entries.length) }) : t("common.save"))
    : armed ? t(untested.length === 1 ? "engines.test_confirm_one" : "engines.test_confirm", { n: String(untested.length) })
    : isNew ? t("engines.test_add_n", { n: String(untested.length) }) : t("engines.test_save"));
  const percent = $derived(run ? Math.round((run.done / Math.max(run.total, 1)) * 100) : 0);
  const time = (c: EngineCheck) => (c.elapsed_ms / 1000).toFixed(1);
</script>

{#snippet modelFields(e: Entry)}
  {@const s = e.sel}
  {@const hint = suggestedIn(listed ?? [], preset?.suggested?.[e.role])}
  {@const modes = e.role === "asr" ? modesOf(pick, services, chosen(e)) : []}
  <div class="fields">
    <span class="lab">{t("engines.model")}{#if hasList && !isNew}<span class="req">{t("common.required")}</span>{/if}</span>
    <div>
      {#if !isNew && hasList && listed}
        <Select
          variable width="100%" label={t("engines.model")} value={s.typed ? OTHER_MODEL : s.model}
          onchange={(v) => { s.typed = v === OTHER_MODEL; if (!s.typed) s.model = v; disarm(); }}
          options={[...(listed ?? []).map((m) => ({ value: m, label: m === hint ? `${m}（${t("engines.suggested")}）` : m })), { value: OTHER_MODEL, label: t("engines.model_other") }]}
        />
        {#if s.typed}<Fold><input type="text" class="typed" bind:value={s.typedModel} aria-label={t("engines.model")} oninput={disarm} /></Fold>{/if}
      {:else if s.typed || !hasList}
        <input type="text" bind:value={s.typedModel} aria-label={t("engines.model")} oninput={disarm} />
        <small>{t("engines.model_type_help")}</small>
      {:else}
        <p class="fixed">{s.model}</p>
      {/if}
    </div>
    {#if both}
      <span class="lab">{t("engines.role_field")}</span>
      <div>
        <Segmented label={t("engines.role_field")} bind:value={() => e.role, (v) => { e.role = v; disarm(); }} options={(["asr", "llm"] as Role[]).map((r) => ({ value: r, label: t(`engines.role.${r}`) }))} />
      </div>
    {/if}
    <label for="f-label-{e.key}-{uid}">{t("engines.label")}</label>
    <div>
      <input id="f-label-{e.key}-{uid}" type="text" bind:value={s.label} placeholder={isNew ? shownName(e) : defaultLabel(e)} />
    </div>
    {#if modes.length}
      <span class="lab">{t("engines.transcribe_mode")}</span>
      <div>
        <Choices
          label={t("engines.transcribe_mode")}
          bind:value={() => s.mode ?? modes[0], (v) => { s.mode = v; disarm(); }}
          options={modes.map((m) => ({ value: m, label: t(`engines.transcribe_mode.${m}`), help: t(`engines.transcribe_mode.${m}_help`), info: m === modes[0] ? undefined : t(`engines.transcribe_mode.${m}_info`, { name: serviceName }) }))}
        />
        {#if modeFor(e) === "smart"}<Fold><small class="mode-note">{t("engines.transcribe_mode.smart_note")}</small></Fold>{/if}
        {#if wordsOffered(e)}<Fold>
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
    {#if takesEffort(pick, services, e.role, chosen(e))}
      <span class="lab">{t("engines.effort")}</span>
      <div>
        <Select
          width="100%" label={t("engines.effort")} value={s.effort ?? ""} onchange={(v) => { s.effort = v || null; disarm(); }}
          options={EFFORTS.map((v) => ({ value: v ?? "", label: t(`engines.effort.${v ?? "default"}`) }))}
        />
        <small>{t("engines.effort_help")}</small>
      </div>
    {/if}
    {#if shown[e.key] && (!isNew || !shown[e.key].ok)}
      {@const c = shown[e.key]}
      <Fold><div class="result" role="status">
        {#if c.ok}
          <ServiceAnswer tone="ok" text={t("engines.check_ok_in", { time: time(c) })} />
        {:else}
          <ServiceAnswer tone="bad" text={c.status ? t("engines.check_failed_status", { status: String(c.status) }) : t("engines.check_failed")} body={checkBody(c)} detail={c.detail} />
        {/if}
      </div></Fold>
    {/if}
  </div>
{/snippet}

{#snippet entryRow(e: Entry)}
  {@const st = statusOf(e)}
  <div class="entry" data-entry={e.key}>
    <div class="entry-h">
      <button type="button" class="entry-t" aria-expanded={e.open} onclick={() => (e.open = !e.open)}>
        <span class="chev" class:open={e.open}><Icon name="chevron-right" size={14} /></span>
        <span class="entry-n"><b>{shownName(e)}</b>{#if summary(e)}<small>{summary(e)}</small>{/if}</span>
      </button>
      {#if st === "run"}<span class="st">{t("engines.entry_running")}</span>
      {:else if st === "wait"}<span class="st">{t("engines.entry_waiting")}</span>
      {:else if st === "ok"}<span class="st ok">{t("engines.entry_passed", { time: time(shown[e.key]) })}</span>
      {:else if st === "bad"}<span class="st bad">{shown[e.key].status ? t("engines.entry_failed_status", { status: String(shown[e.key].status) }) : t("engines.entry_failed")}</span>{/if}
      {#if !run}<button type="button" class="btn icon-btn x" aria-label={t("engines.entry_remove")} use:tip={t("engines.entry_remove")} onclick={() => dropEntry(e)}><Icon name="x" size={14} /></button>{/if}
    </div>
    {#if e.open}<Fold><div class="entry-b" inert={!!run}>{@render modelFields(e)}</div></Fold>{/if}
  </div>
{/snippet}

{#snippet deleteBody()}
  {#if initial}
    {#if isDefaultEngine}
      <p>{t("engines.delete_body", { name: initial.label })}</p>
      <p>{t("engines.delete_is_default", { kind: t(`engines.group.${role}`) })}</p>
    {:else}
      <RemovalNote engines={[initial]} {role} {presets} bind:moveTo />
    {/if}
  {/if}
{/snippet}
{#snippet deleteActions()}
  <button type="button" class="btn" onclick={() => (confirmDelete = false)} disabled={busy}>{t("common.cancel")}</button>
  <button type="button" class="btn danger" onclick={remove} disabled={busy || isDefaultEngine}>{t("engines.delete")}</button>
{/snippet}

<Modal bind:this={dialog} bind:open size={isNew ? "lg" : "md"} grow={isNew} title={isNew ? t("engines.add_external") : t("engines.edit_title")} {beforeclose} {dirty} discardBody={t("engines.discard_body")}
  question={confirmDelete ? { title: t("engines.delete_title"), no: () => { if (!busy) confirmDelete = false; }, body: deleteBody, actions: deleteActions } : null}>
    {#if loaded}<div class="form" class:filled={isNew}>
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
        {#if probe && probe.state !== "ok"}<Fold><p class="probe {probe.state}" role="status">{#if probe.state === "bad"}<b>{t("engines.not_connected")}</b>{/if}{probe.text}</p></Fold>{/if}
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
        {#if !isNew}
          {#each entries as e (e.key)}{@render modelFields(e)}{/each}
        {:else}
          <div class="adder">
            {#if both}
              <span class="lab add-as">{t("engines.add_as")}<InfoTip info={t("engines.add_as_info")} /></span>
              <Segmented label={t("engines.add_as")} bind:value={addRole} options={(["asr", "llm"] as Role[]).map((r) => ({ value: r, label: t(`engines.role.${r}`) }))} />
            {/if}
            <div class="pick">
              <Dropdown label={t("engines.add_model")} search={wantsSearch((listed ?? []).length, true)} bind:query={modelQuery} keepQuery disabled={!!run}>
                {#snippet button()}<span class="ph">{t("engines.add_model")}</span>{/snippet}
                {#snippet list(close)}
                  {#each shownModels as m (m)}
                    {@const n = addedOf(m)}
                    {@const facts = modelFacts(m)}
                    <MenuOption onclick={() => { close(); addEntry(m); }}>
                      <span class="mline"><span class="mid">{m}</span>{#if n}<Tag>{t("engines.added_count", { n: String(n) })}</Tag>{/if}</span>
                      {#if facts}<small class="facts">{facts}</small>{/if}
                    </MenuOption>
                  {/each}
                  {#if !modelQuery.trim() || shownModels.length === 0}
                    <MenuOption onclick={() => { close(); addEntry(null); }}>{t("engines.model_other")}</MenuOption>
                  {/if}
                {/snippet}
              </Dropdown>
            </div>
          </div>
          <div class="list-h">
            <span>{t("engines.to_add", { n: String(entries.length) })}</span>
            {#if entries.length && !run}<button type="button" class="link" class:ask={clearArmed} onclick={clearAll}>{t(clearArmed ? "engines.remove_all_confirm" : "engines.remove_all")}</button>{/if}
          </div>
          <div class="entries">
            <!-- Every part of the list slides, so the centred dialog grows and shrinks smoothly. -->
            {#if entries.length === 0}
              <Fold><p class="empty">{t("engines.to_add_empty")}</p></Fold>
            {:else}<Fold>
              {#each (both ? ["asr", "llm"] : [role]) as Role[] as r (r)}
                {@const mine = entries.filter((e) => e.role === r)}
                {#if mine.length}<Fold>
                  {#if both}<p class="grp">{t(`engines.group.${r}`)}（{mine.length}）</p>{/if}
                  {#each mine as e (e.key)}<Fold>{@render entryRow(e)}</Fold>{/each}
                </Fold>{/if}
              {/each}
            </Fold>{/if}
          </div>
        {/if}
        {#if untested.length > 0 && !run}
          <p class="cost"><Icon name="info" size={14} /><span>{costText}</span></p>
        {/if}
      {/if}
    </div>{/if}

  {#snippet actions()}
    {#if step === "connect"}
      {#if probe?.state === "bad"}<button type="button" class="btn quiet lead" onclick={skipConnect}>{t("engines.continue_anyway")}</button>{/if}
      <button type="button" class="btn" onclick={cancel}>{t("common.cancel")}</button>
      <button type="button" class="btn primary" onclick={() => connect(true)} disabled={!canConnect || probe?.state === "wait"}>{t("engines.connect")}</button>
    {:else}
      {#if !isNew}<button type="button" class="btn quiet danger lead" onclick={askDelete} disabled={busy}>{t("engines.delete")}</button>{/if}
      {#if !run}<button type="button" class="btn quiet" class:lead={isNew} onclick={() => { disarm(); step = "connect"; }} disabled={busy}><Icon name="chevron-left" size={14} />{t(isNew ? "engines.back" : "engines.change_connection")}</button>{/if}
      {#if run}
        <div class="runline" role="status">
          <b>{t("engines.batch_real", { done: String(Math.min(run.done + 1, run.total)), total: String(run.total) })}</b>
          <span class="bar"><i style:width="{percent}%"></i></span>
          {#if run.passed}<span class="ok">{t("engines.batch_passed", { n: String(run.passed) })}</span>{/if}
          {#if run.failed}<span class="bad">{t("engines.batch_failed", { n: String(run.failed) })}</span>{/if}
        </div>
        <button type="button" class="btn" onclick={() => (stopping = true)}>{t("engines.batch_stop")}</button>
      {:else if isNew && failedCount > 0 && ready.length > 0}
        <button type="button" class="btn" onclick={cancel}>{t("common.cancel")}</button>
        <button type="button" class="btn" class:ask={armed} onclick={testAndSave} disabled={busy || !canSave}>{armed ? primaryText : t("engines.retest_n", { n: String(untested.length) })}</button>
        <button type="button" class="btn primary" onclick={() => save(ready)} disabled={busy}>{t("engines.add_passed_n", { n: String(ready.length) })}</button>
      {:else}
        <button type="button" class="btn" onclick={cancel}>{t("common.cancel")}</button>
        <button type="button" class="btn primary" class:ask={armed} onclick={testAndSave} disabled={busy || !canSave}>{busy && untested.length > 0 ? t("engines.testing") : primaryText}</button>
      {/if}
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
  .adder { display: flex; align-items: center; gap: 10px; }
  .add-as { display: inline-flex; align-items: center; gap: 4px; flex: none; color: var(--ink-2); font-size: var(--text-sm); white-space: nowrap; }
  .pick { flex: 1; min-width: 0; }
  .ph { flex: 1; min-width: 0; color: var(--ink-3); font-weight: 400; }
  .mline { display: flex; align-items: center; gap: 8px; }
  .mid { min-width: 0; font-family: var(--font-mono, ui-monospace, Menlo, monospace); font-size: var(--text-sm); overflow-wrap: anywhere; }
  .facts { display: block; color: var(--ink-2); font-size: var(--text-xs); }
  .list-h { display: flex; align-items: center; justify-content: space-between; margin: 2px 2px -6px; font-size: var(--text-xs); font-weight: 600; color: var(--ink-3); }
  .list-h .link { margin: 0; font-size: var(--text-xs); font-weight: 500; }
  .list-h .link.ask { color: var(--live); font-weight: 600; }
  /* The adding form grows with its list up to the window's height; then only the list of engines scrolls. */
  .form.filled { flex: 0 1 auto; min-height: 0; }
  .filled .entries { flex: 0 1 auto; min-height: 0; overflow-y: auto; }
  .entries { border: 1px solid var(--line); border-radius: 12px; overflow: hidden; }
  .entries .empty { margin: 0; padding: 28px 16px; text-align: center; color: var(--ink-3); font-size: var(--text-sm); }
  .grp { margin: 0; padding: 7px 14px; background: var(--sunken); border-top: 1px solid var(--line); font-size: var(--text-xs); font-weight: 600; color: var(--ink-2); }
  .grp:first-child { border-top: 0; }
  .entry { border-top: 1px solid var(--line); }
  .entries :global(.fold:first-child > .entry), .grp + :global(.fold > .entry) { border-top: 0; }
  .entries :global(.fold + .fold > .grp) { border-top: 1px solid var(--line); }
  .entry-h { display: flex; align-items: center; gap: 10px; padding: 6px 8px 6px 4px; }
  .entry-t { flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px; padding: 4px 6px; border: 0; border-radius: 8px; background: none; color: var(--ink); text-align: left; cursor: default; }
  .entry-t:hover { background: var(--sunken); }
  .chev { display: inline-flex; flex: none; color: var(--ink-3); transition: transform .2s; }
  .chev.open { transform: rotate(90deg); }
  @media (prefers-reduced-motion: reduce) { .chev { transition: none; } }
  .entry-n { min-width: 0; }
  .entry-n b { display: block; font-size: var(--text-sm); font-weight: 600; overflow-wrap: anywhere; }
  .entry-n small { display: block; color: var(--ink-2); font-size: var(--text-xs); overflow-wrap: anywhere; }
  .st { flex: none; font-size: var(--text-xs); color: var(--ink-3); white-space: nowrap; }
  .st.ok { color: var(--ok); }
  .st.bad { color: var(--live); }
  .x { width: 28px; height: 28px; padding: 0; justify-content: center; color: var(--ink-3); border-color: transparent; background: none; }
  .x:hover { color: var(--ink); }
  .entry-b { padding: 4px 14px 14px 30px; }
  .fixed { margin: 0; padding-top: 7px; font-family: var(--font-mono, ui-monospace, Menlo, monospace); font-size: var(--text-sm); overflow-wrap: anywhere; }
  .runline { flex: 1; min-width: 0; display: flex; align-items: center; gap: 12px; margin-right: auto; font-size: var(--text-sm); color: var(--ink-2); }
  .runline b { color: var(--ink); font-weight: 600; white-space: nowrap; }
  .runline .bar { flex: 1; min-width: 60px; max-width: 280px; height: 6px; border-radius: 3px; background: var(--sunken); overflow: hidden; }
  .runline .bar i { display: block; height: 100%; border-radius: 3px; background: var(--accent); transition: width .2s; }
  .runline .ok { color: var(--ok); white-space: nowrap; }
  .runline .bad { color: var(--live); white-space: nowrap; }
  .fields > :global(.fold) { grid-column: 2; }
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
