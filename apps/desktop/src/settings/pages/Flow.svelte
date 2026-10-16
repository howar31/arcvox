<script lang="ts">
  import { onMount, untrack } from "svelte";
  import {
    errorMessage, historyLast, listMicrophones, modelsInstalled, modelsManifestNow, onHistoryChanged, profilesAddPreset,
    profilesPresets, setDefaultProfile, upsertProfile, type InstalledModel, type LastRun, type ManifestView, type PresetView, type Profile,
  } from "../../lib/api";
  import { appName, ensureAppNames } from "../../lib/apps.svelte";
  import { appliesDictionary, PUNCTUATION_CONFIGS, RULE_KEYS, rulesAt, SCRIPT_CONFIGS, SCRIPT_GROUPS, scriptAt, scriptLabel, selectedProfile, withDictionary, withRule, withScriptAt, withScriptPunctuation, type RuleKey, type ScriptPass } from "../../lib/flowEdit";
  import { cellLine, cellTime, runHeadline, STAGE_IDS, stageOpen, type StageId } from "../../lib/flowLast";
  import { rememberProfile, rememberStage, storedProfile, storedStage } from "../../lib/stageMemory";
  import { guard } from "../../lib/guard.svelte";
  import { t } from "../../lib/i18n";
  import { isImeKey } from "../../lib/ime";
  import { appsSummary, freshName, withLlm } from "../../lib/profileEdit";
  import { anyFallback, FOLLOW, hasLlmStep, isFallback, resolveEngines, usableWith } from "../../lib/resolve";
  import { encoderMissing, runnable } from "../../lib/engines";
  import { go } from "../../lib/route";
  import { engineLine, micLine, outputLine, type StageLine } from "../../lib/stages";
  import { client, config } from "../../lib/store.svelte";
  import Chip from "../../lib/ui/Chip.svelte";
  import Fold from "../../lib/ui/Fold.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import PageLink from "../../lib/ui/PageLink.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import SaveBar from "../../lib/ui/SaveBar.svelte";
  import Switch from "../../lib/ui/Switch.svelte";
  import { toast } from "../../lib/ui/toast.svelte";
  import AddPerApp from "./AddPerApp.svelte";
  import EnginePicker from "./flow/EnginePicker.svelte";
  import LastPanel from "./flow/LastPanel.svelte";
  import ManageProfiles from "./flow/ManageProfiles.svelte";
  import ProfilePicker from "./flow/ProfilePicker.svelte";
  import StageRail, { type Cell } from "./flow/StageRail.svelte";

  /** `arg` is the id of the profile to show; without it the default profile is shown. */
  let { arg }: { arg: string | null } = $props();

  const LANGS = ["zh-TW", "en", "ja"];
  const LANG_KEYS: Record<string, string> = { "zh-TW": "profiles.lang.zh_tw", en: "profiles.lang.en", ja: "profiles.lang.ja" };

  // The stage shown first is the one shown when the page was left last.
  let stage = $state<StageId>(storedStage());
  $effect(() => { rememberStage(stage); });
  let managing = $state(false);
  // The profile's newest dictation with its stages: undefined while it is being read, null when there is none.
  let last = $state<LastRun | null | undefined>(undefined);
  let lastFor = "";
  async function loadLast(id: string) {
    try {
      const run = await historyLast(id);
      if (lastFor === id) last = run;
    } catch {
      if (lastFor === id) last = null;
    }
  }
  let draft = $state<Profile | null>(null);
  // The stored profile the draft was last copied from, and its id.
  let synced = "";
  let syncedId = "";
  let saving = $state(false);
  let problem = $state<string | undefined>(undefined);
  let adding = $state(false);
  let busy = $state(false);
  let presets = $state<PresetView[]>([]);
  let mics = $state<string[]>([]);
  let models = $state<InstalledModel[] | null>(null);
  let manifest = $state<ManifestView | null>(null);
  // Typed but not added yet: counts as a change, and Save adds it first.
  let newApp = $state("");
  let newUrl = $state("");
  let newRaw = $state("");
  let newRefined = $state("");

  onMount(() => {
    const off = onHistoryChanged(() => { if (lastFor) loadLast(lastFor); });
    listMicrophones().then((m) => { mics = m; }).catch(() => { mics = []; });
    modelsInstalled().then((m) => { models = m; }).catch(() => { models = null; });
    modelsManifestNow().then((m) => { manifest = m; }).catch(() => { manifest = null; });
    profilesPresets().then((p) => { presets = p; }).catch((e) => toast.error(t("profiles.presets_failed", { detail: errorMessage(e) })));
    return () => { off.then((u) => u()); };
  });

  // Another profile: read its newest dictation; the cells stay empty until it is known.
  $effect(() => {
    const id = source?.id;
    if (!id) return;
    untrack(() => {
      if (lastFor === id) return;
      lastFor = id;
      last = undefined;
      loadLast(id);
    });
  });

  // Whether a profile is the default is changed on its own ("Make default"), not through the draft.
  const comparable = (p: Profile) => JSON.stringify({ ...p, is_default: false });

  // The default profile first, the others in their stored order.
  const profiles = $derived([...(config.value?.profiles ?? [])].sort((a, b) => Number(b.is_default) - Number(a.is_default)));
  // Without a profile in the address, the one shown when the page was left; a deleted one gives way to the default.
  const source = $derived(selectedProfile(config.value, arg ?? storedProfile()));
  $effect(() => { if (source) rememberProfile(source.id); });
  const providers = $derived(config.value?.providers ?? []);
  const asrChoices = $derived(providers.filter((p) => p.capabilities.transcribe));
  const llmChoices = $derived(providers.filter((p) => p.capabilities.chat));
  const installed = $derived.by(() => {
    const ok = runnable(models ?? []);
    const deaf = (models ?? []).filter(encoderMissing).flatMap((m) => [m.id, m.path]);
    return { ids: models ? new Set(ok.map((m) => m.id)) : null, paths: new Set(ok.map((m) => m.path)), deaf: new Set(deaf) };
  });
  // The engines the draft runs with now: its own when they can run, else the default pair.
  const resolved = $derived(draft ? resolveEngines(config.value, draft, usableWith(installed.ids, installed.paths)) : null);
  // The stored profiles that run on the default engine because an engine they name cannot run.
  const faulty = $derived.by(() => {
    const usable = usableWith(installed.ids, installed.paths);
    return new Set(profiles.filter((p) => anyFallback(resolveEngines(config.value, p, usable))).map((p) => p.id));
  });
  const defaultName = (role: "asr" | "llm") => providers.find((p) => p.id === config.value?.default_engines[role])?.label ?? config.value?.default_engines[role] ?? "";
  const followChoice = (role: "asr" | "llm") => ({ name: t("flow.follow_default"), help: t("flow.follow_default_help", { name: defaultName(role) }) });
  // The engine the draft's template was tuned with; none for an untuned template or a profile without one.
  const suggested = $derived.by(() => {
    const preset = presets.find((p) => p.id === draft?.preset);
    return { asr: preset?.suggested_asr ?? null, llm: preset?.suggested_llm ?? null };
  });
  /** The fallback note of a stage: the engine the draft names cannot run, the default is used. */
  function onDefault(role: "asr" | "llm"): { wanted: string; using: string; modelId: string | null } | null {
    const c = role === "asr" ? resolved?.asr : resolved?.llm;
    if (!c || !isFallback(c)) return null;
    const wanted = providers.find((p) => p.id === c.source.wanted);
    const modelId = wanted?.kind === "embedded" && wanted.model_id && manifest?.models.some((m) => m.id === wanted.model_id) ? wanted.model_id : null;
    return { wanted: wanted?.label ?? c.source.wanted, using: providers.find((p) => p.id === c.id)?.label ?? c.id, modelId };
  }

  // Copy the stored profile into the draft: always for another profile, otherwise only while nothing is edited.
  $effect(() => {
    if (!source) { draft = null; return; }
    const next = comparable(source);
    const id = source.id;
    untrack(() => {
      if (draft === null || id !== syncedId || comparable(draft) === synced) {
        draft = structuredClone($state.snapshot(source)) as Profile;
        synced = next;
        if (id !== syncedId) { newApp = ""; newUrl = ""; newRaw = ""; newRefined = ""; }
        syncedId = id;
      }
    });
  });

  const pending = $derived(!!(newApp.trim() || newUrl.trim() || newRaw.trim() || newRefined.trim()));
  const halfExample = $derived(!newRaw.trim() !== !newRefined.trim());
  const dirty = $derived(draft !== null && source !== null && (pending || comparable(draft) !== comparable(source)));

  $effect(() => {
    guard.set(dirty);
    return () => guard.set(false);
  });
  $effect(() => {
    // A new edit clears the previous validation message.
    JSON.stringify([draft, newRaw, newRefined]);
    problem = undefined;
  });
  $effect(() => { ensureAppNames(profiles.flatMap((p) => p.matcher.app_ids)); });

  const lines = $derived.by((): Record<"mic" | "asr" | "llm" | "out", StageLine> | null => {
    if (!draft) return null;
    return {
      mic: micLine(client.value?.microphone ?? "builtin", mics),
      asr: engineLine(providers, resolved?.asr.id ?? null, installed),
      llm: engineLine(providers, resolved?.llm?.id ?? null, installed),
      out: outputLine(draft, client.value?.insertion.default),
    };
  });

  // What each stage did in the last dictation, computed when that run changes, not with every edit of the draft.
  const runLines = $derived.by((): Record<StageId, string> => {
    const run = last?.stages ?? null;
    return Object.fromEntries(STAGE_IDS.map((id) => [id, cellLine(id, run)])) as Record<StageId, string>;
  });
  const runTimes = $derived.by((): Record<StageId, string | null> => {
    const run = last?.stages ?? null;
    return Object.fromEntries(STAGE_IDS.map((id) => [id, cellTime(id, run)])) as Record<StageId, string | null>;
  });

  // One cell per stage: the stage's name, its current setting, and what it did in the last dictation.
  const cells = $derived.by((): Cell[] => {
    if (!draft || !lines) return [];
    const passLabel = (pass: ScriptPass) => {
      const script = scriptAt(draft!, pass);
      return script ? scriptLabel(script.config) : t("flow.script_none");
    };
    const on = rulesAt(draft);
    const ruleCount = on ? RULE_KEYS.filter((k) => k !== "dictionary" && on[k]).length : 0;
    const rules = !on || (!on.dictionary && ruleCount === 0) ? t("flow.rules_none") : on.dictionary ? t("flow.rules_meta", { n: String(ruleCount) }) : t("flow.rules_meta_nodict", { n: String(ruleCount) });
    const cell = (id: StageId, name: string, setting: string, warn = false, unknown = false): Cell =>
      ({ id, name, setting, line: runLines[id], time: runTimes[id], open: stageOpen(id, draft!), warn, unknown, problem: problemOf(id) });
    return [
      cell("capture", t("home.stage.capture"), lines.mic.name, lines.mic.missing),
      cell("trim", t("flow.stage.trim"), t("flow.trim_meta")),
      cell("asr", t("home.stage.asr"), lines.asr.name, lines.asr.missing, !lines.asr.known),
      cell("opencc1", t("flow.stage.opencc"), passLabel(1)),
      cell("llm", t("home.stage.llm"), lines.llm.name, lines.llm.missing, !lines.llm.known),
      cell("opencc2", t("flow.stage.opencc"), passLabel(2)),
      cell("rules", t("flow.stage.rules"), rules),
      cell("insert", t("flow.stage.insert"), lines.out.name),
      cell("record", t("flow.stage.record"), t("flow.record_meta")),
    ];
  });
  const current = $derived(cells.find((c) => c.id === stage));
  /** The problem mark of an engine stage that runs on the default because the engine named for it cannot run. */
  function problemOf(id: StageId): string | undefined {
    const c = id === "asr" ? resolved?.asr : id === "llm" ? resolved?.llm : null;
    return c && isFallback(c) ? t("flow.profile_fallback") : undefined;
  }
  // Opening a profile with a problem shows the stage it is in: from the list, a link or the menu bar.
  let shownProfile: string | null = null;
  $effect(() => {
    const p = source;
    // Wait for the model files: before they are read every engine counts as able to run.
    if (!p || p.id === shownProfile || installed.ids === null) return;
    shownProfile = p.id;
    untrack(() => {
      const r = resolveEngines(config.value, p, usableWith(installed.ids, installed.paths));
      if (isFallback(r.asr)) stage = "asr";
      else if (isFallback(r.llm)) stage = "llm";
    });
  });
  // The right column's title repeats the setting, and for an engine adds whether it is ready.
  const titleMeta = $derived.by(() => {
    if (!current || !lines) return "";
    const meta = stage === "asr" ? lines.asr.meta : stage === "llm" ? lines.llm.meta : stage === "capture" ? lines.mic.meta : "";
    return meta && meta !== current.setting ? `${current.setting} · ${meta}` : current.setting;
  });

  /** Where a profile applies, in one sentence. */
  function applies(p: Profile): string {
    if (p.is_default) return t("flow.applies_default");
    const names = [...p.matcher.app_ids.map(appName), ...p.matcher.url_patterns];
    if (names.length === 0) return t("flow.applies_unused");
    const { shown, more } = appsSummary(names);
    const list = shown.join(t("home.list_sep"));
    return t("flow.applies_apps", { apps: more > 0 ? t("home.apps_more", { names: list, count: String(more) }) : list });
  }

  /** The engine a profile names when it is not among the choices for that stage. */
  function stale(id: string | null, choices: typeof providers): { name: string; help: string } | undefined {
    if (id === null || choices.some((p) => p.id === id)) return undefined;
    const known = providers.find((p) => p.id === id);
    return known
      ? { name: known.label, help: t("flow.engine_unusable") }
      : { name: id, help: t("flow.engine_missing") };
  }

  function pickMic(select: HTMLSelectElement) {
    if (!client.value) return;
    client.save({ ...$state.snapshot(client.value), microphone: select.value })
      .then(() => toast.ok(t("common.saved")))
      .catch((e) => { select.value = client.value?.microphone ?? "builtin"; toast.error(errorMessage(e)); });
  }

  const snapshot = () => $state.snapshot(draft) as Profile;
  function addApp(id: string) {
    if (draft && !draft.matcher.app_ids.includes(id)) draft.matcher.app_ids.push(id);
  }
  function addUrl() {
    const url = newUrl.trim();
    if (!draft || !url) return;
    if (!draft.matcher.url_patterns.includes(url)) draft.matcher.url_patterns.push(url);
    newUrl = "";
  }
  function addExample() {
    if (!draft || !newRaw.trim() || !newRefined.trim()) return;
    draft.few_shot.push({ raw: newRaw, refined: newRefined });
    newRaw = "";
    newRefined = "";
  }

  async function save() {
    if (!draft || !source) return;
    if (!draft.name.trim()) { problem = t("profiles.name_required"); return; }
    if (halfExample) { problem = t("profiles.example_incomplete"); stage = "llm"; return; }
    if (newApp.trim()) { addApp(newApp.trim()); newApp = ""; }
    addUrl();
    addExample();
    const next = snapshot();
    next.is_default = source.is_default;
    saving = true;
    try {
      config.value = await upsertProfile(next);
      // The draft now equals what is stored.
      synced = comparable(next);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      saving = false;
    }
  }
  function revert() {
    if (!source) return;
    draft = structuredClone($state.snapshot(source)) as Profile;
    synced = comparable(source);
    newApp = ""; newUrl = ""; newRaw = ""; newRefined = "";
  }

  // The add dialog: the template, the new profile's name and whether it becomes the default.
  let addPreset = $state("");
  let addName = $state("");
  // Set once the name is typed; until then the name follows the chosen template.
  let addRenamed = $state(false);
  let addDefault = $state(false);
  const takenNames = () => (config.value?.profiles ?? []).map((p) => p.name);
  function openAdd() {
    revert();
    addPreset = presets[0]?.id ?? "";
    addName = addPreset ? freshName(t(`profiles.preset.${addPreset}`), takenNames()) : "";
    addRenamed = false;
    addDefault = false;
    adding = true;
  }
  function choosePreset(id: string) {
    addPreset = id;
    if (!addRenamed) addName = freshName(t(`profiles.preset.${id}`), takenNames());
  }

  /** Adds a copy of the chosen template under the given name, makes it the default when asked, and shows it. */
  async function create() {
    if (!config.value || !addPreset || !addName.trim()) return;
    busy = true;
    const before = new Set(config.value.profiles.map((p) => p.id));
    try {
      let next = await profilesAddPreset(addPreset, addName.trim());
      const created = next.profiles.find((p) => !before.has(p.id));
      if (created && addDefault) next = await setDefaultProfile(created.id);
      config.value = next;
      adding = false;
      toast.ok(t("common.saved"));
      if (created) go("flow", created.id);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      busy = false;
    }
  }
</script>

<PageHead title={t("nav.flow")} description={t("flow.description")} />

{#snippet fallbackNote(role: "asr" | "llm")}
  {@const fb = onDefault(role)}
  {#if fb}
    <p class="missing fallback">
      <span>{t("flow.on_default", { wanted: fb.wanted, using: fb.using })}</span>
      <button type="button" class="btn" onclick={() => guard.request(() => go("engines", role))}>{t(fb.modelId ? "flow.to_download" : "flow.to_engines")}</button>
    </p>
  {/if}
{/snippet}

{#if config.value && draft && source && lines}
  <div class="pick">
    <ProfilePicker
      {profiles} current={source.id} name={draft.name.trim() || source.name} {applies} {faulty}
      onpick={(id) => guard.request(() => go("flow", id))}
      onadd={() => guard.request(openAdd)}
      onmanage={() => guard.request(() => (managing = true))}
    />
  </div>

  {#if source.is_default}
    <Group title={t("profiles.applies")}>
      <Row label={t("profiles.everywhere")} help={t("flow.default_statement")} />
    </Group>
  {:else}
    <Group title={t("profiles.applies")}>
      <Row label={t("profiles.apps_label")}>
        <div class="chips end-chips">
          {#each draft.matcher.app_ids as id, i (i)}
            <Chip onremove={() => draft!.matcher.app_ids.splice(i, 1)}><span title={id}>{appName(id)}</span></Chip>
          {:else}
            <span class="sub">{t("profiles.no_apps")}</span>
          {/each}
        </div>
      </Row>
      <AddPerApp onadd={addApp} bind:value={newApp} />
    </Group>
    <Group>
      <Row label={t("profiles.urls")} help={t("profiles.urls_help")}>
        <input type="text" class="add-input" bind:value={newUrl} placeholder={t("profiles.url_placeholder")} aria-label={t("profiles.urls")} onkeydown={(e) => { if (e.key === "Enter" && !isImeKey(e)) addUrl(); }} />
        <button type="button" class="btn" disabled={!newUrl.trim()} onclick={addUrl}><Icon name="plus" size={14} />{t("common.add")}</button>
      </Row>
      {#if draft.matcher.url_patterns.length}<Fold>
        <Row>
          <div class="chips">
            {#each draft.matcher.url_patterns as u, i (i)}<Chip onremove={() => draft!.matcher.url_patterns.splice(i, 1)}>{u}</Chip>{/each}
          </div>
        </Row>
      </Fold>{/if}
    </Group>
  {/if}

  <div class="gt">
    <span>{t("flow.stages")}</span>
    {#if last}<span class="r">{runHeadline(last.row, last.stages, new Date())}</span>{/if}
  </div>
  <div class="split">
    <!-- The rail stays in view while the right column scrolls; at the window's minimum height it fits whole. -->
    <div class="railwrap"><StageRail {cells} selected={stage} onpick={(id) => (stage = id)} label={t("flow.stages")} /></div>
    <div class="right">
      {#if current}
        <div class="ttl"><b>{current.name}</b><span class:warn={current.warn && current.open} class:unknown={current.unknown}>{titleMeta}</span></div>
      {/if}
      {#if last !== undefined}
        <LastPanel id={stage} stages={last?.stages ?? null} row={last?.row ?? null} />
      {/if}

      {#if stage === "capture"}
        {@const mic = client.value?.microphone ?? "builtin"}
        <Group title={t("home.stage.capture")}>
          <Row label={t("general.input_device")} help={t("flow.mic_shared")}>
            <select class="field" value={mic} aria-label={t("general.input_device")} onchange={(e) => pickMic(e.currentTarget)}>
              <option value="builtin">{t("general.mic_builtin")}</option>
              <option value="default">{t("general.mic_default")}</option>
              {#each mics as m (m)}<option value={m}>{m}</option>{/each}
              {#if lines.mic.missing}<option value={mic} disabled>{mic}</option>{/if}
            </select>
          </Row>
        </Group>
      {:else if stage === "trim"}
        <Group title={t("flow.stage.trim")}>
          <Row label={t("flow.trim_meta")} help={t("flow.trim_help")} />
        </Group>
      {:else if stage === "asr"}
        <div class="bar"><h2>{t("flow.asr_engines")}</h2><button type="button" class="btn quiet" onclick={() => guard.request(() => go("engines", "asr"))}>{t("flow.manage")}</button></div>
        <!-- The reason for the stage's problem mark, above the engines to choose from. -->
        {@render fallbackNote("asr")}
        <EnginePicker
          engines={asrChoices} selected={draft.asr_provider ?? FOLLOW} label={t("flow.asr_engines")} {manifest} {installed} models={models ?? []} follow={followChoice("asr")} suggested={suggested.asr}
          stale={stale(draft.asr_provider, asrChoices)} onpick={(id) => { draft!.asr_provider = id === FOLLOW ? null : id; }}
        />
        {#if lines.asr.missing}
          <p class="missing"><span>{t("flow.model_missing")}</span><button type="button" class="btn" onclick={() => guard.request(() => go("engines", "asr"))}>{t("flow.to_download")}</button></p>
        {/if}
        <Group title={t("flow.asr_settings")}>
          <Row label={t("profiles.language_hint")} help={t("profiles.language_help")}>
            <select class="field" bind:value={() => draft!.language_hint ?? "", (v) => (draft!.language_hint = v || null)} aria-label={t("profiles.language_hint")}>
              {#each LANGS as l (l)}<option value={l}>{t(LANG_KEYS[l])}</option>{/each}
              {#if draft.language_hint && !LANGS.includes(draft.language_hint)}<option value={draft.language_hint}>{draft.language_hint}</option>{/if}
              <option value="">{t("profiles.lang.auto")}</option>
            </select>
          </Row>
          <Row label={t("flow.asr_hint")} help={t("flow.asr_hint_help")}>
            <Switch bind:checked={draft.asr_dictionary_hint} label={t("flow.asr_hint")} />
          </Row>
        </Group>
      {:else if stage === "llm"}
        <div class="bar"><h2>{t("flow.llm_engines")}</h2><button type="button" class="btn quiet" onclick={() => guard.request(() => go("engines", "llm"))}>{t("flow.manage")}</button></div>
        <!-- The reason for the stage's problem mark, above the engines to choose from. -->
        {@render fallbackNote("llm")}
        <EnginePicker
          engines={llmChoices} selected={hasLlmStep(draft) ? (draft.llm_provider ?? FOLLOW) : null} label={t("flow.llm_engines")} {manifest} {installed} models={models ?? []}
          follow={followChoice("llm")} suggested={suggested.llm} none={{ name: t("home.no_rewrite"), help: t("home.no_rewrite_meta") }}
          stale={stale(draft.llm_provider, llmChoices)} onpick={(id) => (draft = withLlm(snapshot(), id))}
        />
        {#if lines.llm.missing}
          <p class="missing"><span>{t("flow.model_missing")}</span><button type="button" class="btn" onclick={() => guard.request(() => go("engines", "llm"))}>{t("flow.to_download")}</button></p>
        {/if}
        {#if hasLlmStep(draft)}<Fold>
          <div class="bar"><h2>{t("profiles.prompt_addition")}</h2><button type="button" class="btn quiet" onclick={() => guard.request(() => go("prompt"))}>{t("flow.see_prompt")}</button></div>
          <textarea rows="4" bind:value={draft.prompt_addition} aria-label={t("profiles.prompt_addition")}></textarea>
          <p class="help">{t("profiles.prompt_help")}</p>

          <Group title={t("profiles.few_shot")} help={t("profiles.few_shot_help")}>
            {#each draft.few_shot as ex, i (i)}
              <Row>
                <span class="pair-text">{ex.raw}</span>
                <span class="arrow" aria-hidden="true">→</span>
                <span class="pair-text">{ex.refined}</span>
                <span class="end"><button type="button" class="btn quiet icon-btn" aria-label={t("common.remove")} onclick={() => draft!.few_shot.splice(i, 1)}><Icon name="trash" /></button></span>
              </Row>
            {/each}
            <Row>
              <input type="text" class="pair-text" bind:value={newRaw} placeholder={t("profiles.raw")} aria-label={t("profiles.raw")} aria-invalid={problem && halfExample && !newRaw.trim() ? "true" : undefined} />
              <span class="arrow" aria-hidden="true">→</span>
              <input type="text" class="pair-text" bind:value={newRefined} placeholder={t("profiles.refined")} aria-label={t("profiles.refined")} aria-invalid={problem && halfExample && !newRefined.trim() ? "true" : undefined} onkeydown={(e) => { if (e.key === "Enter" && !isImeKey(e)) addExample(); }} />
              <span class="end"><button type="button" class="btn" disabled={!newRaw.trim() || !newRefined.trim()} onclick={addExample}><Icon name="plus" size={14} />{t("common.add")}</button></span>
            </Row>
          </Group>

          <Group title={t("flow.llm_rules")}>
            <Row label={t("flow.llm_terms")} help={t("flow.llm_terms_help")}>
              <Switch bind:checked={draft.llm_dictionary_terms} label={t("flow.llm_terms")} />
            </Row>
            <Row label={t("flow.llm_title")} help={t("flow.llm_title_help")}>
              <Switch bind:checked={draft.llm_window_title} label={t("flow.llm_title")} />
            </Row>
            <Row label={t("flow.llm_short")} help={t("flow.llm_short_help")}>
              <Switch bind:checked={draft.llm_short_fallback} label={t("flow.llm_short")} />
            </Row>
          </Group>
        </Fold>{/if}
      {:else if stage === "opencc1" || stage === "opencc2"}
        {@const pass: ScriptPass = stage === "opencc1" ? 1 : 2}
        {@const script = scriptAt(draft, pass)}
        {@const noRewrite = pass === 2 && !stageOpen("llm", draft)}
        <Group title={t("flow.stage.opencc")}>
          <Row label={t("flow.script")} help={t(pass === 1 ? "flow.script_help" : noRewrite ? "flow.script_help2_off" : "flow.script_help2")}>
            <!-- Sixteen fixed conversions: a list, grouped by the script they produce. A configuration set
                 outside this page is listed by its name and kept until another choice is made. The second
                 pass follows the rewrite; without one it cannot be set. -->
            <select class="field wide" value={script?.config ?? ""} aria-label={t("flow.script")} disabled={noRewrite} onchange={(e) => (draft = withScriptAt(snapshot(), pass, e.currentTarget.value || null))}>
              <option value="">{t("flow.script_none")}</option>
              {#each SCRIPT_GROUPS as g (g.id)}
                <optgroup label={t(`flow.script_group.${g.id}`)}>
                  {#each g.configs as c (c)}<option value={c}>{scriptLabel(c)}</option>{/each}
                </optgroup>
              {/each}
              {#if script && !SCRIPT_CONFIGS.includes(script.config)}<option value={script.config}>{script.config}</option>{/if}
            </select>
          </Row>
          {#if script && PUNCTUATION_CONFIGS.includes(script.config)}<Fold>
            <Row label={t("flow.script_punct")} help={t("flow.script_punct_help")}>
              <Switch bind:checked={() => script.punctuation, (v) => (draft = withScriptPunctuation(snapshot(), pass, v))} label={t("flow.script_punct")} />
            </Row>
          </Fold>{/if}
          <Row label={t("flow.opencc_engine")}>
            {#snippet help()}{t("flow.opencc_engine_before")}<PageLink page="about" label={t("nav.about")} />{t("flow.opencc_engine_after")}{/snippet}
          </Row>
        </Group>
      {:else if stage === "rules"}
        <Group title={t("flow.dictionary_group")}>
          <Row label={t("flow.dictionary")} help={t("flow.dictionary_help")}>
            <Switch bind:checked={() => appliesDictionary(draft!), (v) => (draft = withDictionary(snapshot(), v))} label={t("flow.dictionary")} />
          </Row>
          <Row label={t("flow.rules_edit")} help={t("flow.rules_edit_help")} onclick={() => guard.request(() => go("dictionary"))} />
        </Group>
        <Group title={t("flow.rules_group")}>
          {#each (["trailing_stop", "question_mark", "question_mark_inside", "spacing", "punctuation_width"] as RuleKey[]) as key (key)}
            {@const on = rulesAt(draft!)?.[key] ?? false}
            <Row label={t(`flow.rule.${key}`)} help={t(on ? `flow.rule.${key}_on` : `flow.rule.${key}_off`)}>
              <Switch bind:checked={() => on, (v) => (draft = withRule(snapshot(), key, v))} label={t(`flow.rule.${key}`)} />
            </Row>
          {/each}
        </Group>
      {:else if stage === "insert"}
        <Group title={t("flow.stage.insert")}>
          <Row label={t("flow.insert_row")} help={t("flow.insert_help")}>
            <button type="button" class="btn quiet" onclick={() => guard.request(() => go("recording"))}>{t("flow.to_recording")}</button>
          </Row>
        </Group>
      {:else}
        <Group title={t("flow.stage.record")}>
          <Row label={t("flow.record_history")} onclick={() => guard.request(() => go("history"))} />
          <Row label={t("flow.record_recordings")} onclick={() => guard.request(() => go("general"))} />
        </Group>
      {/if}
    </div>
  </div>

  <SaveBar {dirty} {saving} message={problem} onsave={save} onrevert={revert} />
{/if}

<ManageProfiles bind:open={managing} {profiles} {applies} {presets} />

<Modal bind:open={adding} title={t("flow.add")}>
  <div class:busy inert={busy} aria-busy={busy}>
    <div class="templates" role="radiogroup" aria-label={t("flow.add_template")}>
      {#each presets as p (p.id)}
        <label class="template" class:on={addPreset === p.id}>
          <input type="radio" name="preset" value={p.id} checked={addPreset === p.id} onchange={() => choosePreset(p.id)} />
          <span><b>{t(`profiles.preset.${p.id}`)}</b><small>{t(`profiles.preset.${p.id}.desc`)}</small></span>
        </label>
      {/each}
    </div>
    <div class="choices">
      <Row label={t("flow.add_name")}>
        <input class="field" type="text" bind:value={addName} oninput={() => (addRenamed = true)} aria-label={t("flow.add_name")} />
      </Row>
      <Row label={t("flow.add_default")} help={t(addDefault ? "flow.add_default_on" : "flow.add_default_off")}>
        <Switch bind:checked={addDefault} label={t("flow.add_default")} />
      </Row>
    </div>
  </div>
  {#snippet actions()}
    {#if busy}<span class="adding" role="status">{t("profiles.adding")}</span>{/if}
    <button type="button" class="btn" disabled={busy} onclick={() => (adding = false)}>{t("common.cancel")}</button>
    <button type="button" class="btn primary" disabled={busy || !addPreset || !addName.trim()} onclick={create}>{t("flow.add_create")}</button>
  {/snippet}
</Modal>


<style>
  .pick { display: flex; align-items: center; gap: 10px; }

  .gt { display: flex; justify-content: space-between; align-items: baseline; gap: 12px; margin: 22px 4px 12px; font: 600 var(--text-sm)/1 var(--font); color: var(--ink-2); }
  .gt .r { font-weight: 400; color: var(--ink-3); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  /* The rail can only stick while its container is taller than it, so the split reaches the bottom of the
     scroll area even when the selected stage's settings are short: it takes the page's bottom padding as its own. */
  .split { display: grid; grid-template-columns: 236px minmax(0, 1fr); gap: 16px; align-items: start; min-height: calc(100vh - 28px); margin-bottom: -80px; }
  .railwrap { position: sticky; top: 28px; }
  .right { display: flex; flex-direction: column; min-width: 0; padding-bottom: 80px; }
  .right .ttl { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; margin: 2px 4px 10px; }
  .right .ttl b { font: 700 19px/1.3 var(--font-round); flex: none; }
  .right .ttl span { color: var(--ink-2); font-size: var(--text-sm); text-align: right; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .right .ttl span.warn { color: var(--warn); }
  .right .ttl span.unknown { color: var(--live); }
  /* The first group under the panel keeps the page's usual distance; the panel sits right under the title. */
  .right :global(.section:first-of-type h2) { margin-top: var(--gap-group); }

  .bar { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin: var(--gap-group) 4px 12px; min-height: 20px; }
  .missing { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin: 10px 4px 0; color: var(--warn); font-size: var(--text-sm); }
  .missing.fallback { margin: 0 4px 10px; color: var(--live); }
  .bar h2 { margin: 0; font: 600 var(--text-sm)/1 var(--font); color: var(--ink-2); }
  .bar .btn { margin: -10px -8px -10px 0; }
  .field { width: 240px; flex: none; }
  .field.wide { width: 300px; }
  textarea { display: block; }
  .pair-text { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .arrow { flex: none; color: var(--ink-3); }
  /* Keeps the arrows of the example rows and the add row in one column. */
  .end { width: 92px; flex: none; display: flex; justify-content: flex-end; }
  .end .icon-btn { margin-right: -8px; }
  .icon-btn { width: var(--control); padding: 0; color: var(--ink-2); }
  .icon-btn:hover { color: var(--ink); }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; flex: 1; min-width: 0; }
  .end-chips { flex: 0 1 auto; justify-content: flex-end; max-width: 70%; }
  .busy { opacity: .55; }
  .templates { display: flex; flex-direction: column; gap: 8px; }
  .template { display: flex; align-items: flex-start; gap: 12px; padding: 11px 14px; border: 1px solid var(--line); border-radius: var(--radius); background: var(--surface); color: var(--ink); cursor: default; }
  .template.on { border-color: var(--accent); background: var(--accent-soft); }
  .template input { appearance: none; flex: none; width: 16px; height: 16px; margin: 4px 0 0; border: 1.5px solid var(--ink-3); border-radius: 50%; }
  .template input:checked { border: 5px solid var(--accent); }
  .template input:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .template b { display: block; font-weight: 600; }
  .template small { display: block; color: var(--ink-2); font-size: var(--text-sm); }
  .adding { margin-right: auto; align-self: center; color: var(--ink-2); font-size: var(--text-sm); }
  .choices { margin-top: 8px; border: 1px solid var(--line); border-radius: var(--radius-group); background: var(--surface); color: var(--ink); }
</style>
