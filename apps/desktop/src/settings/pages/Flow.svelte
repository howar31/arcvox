<script lang="ts">
  import { tip } from "../../lib/tip";
  import { onMount, untrack } from "svelte";
  import {
    errorMessage, historyLast, listMicrophones, modelsDownload, modelsInstalled, modelsManifestNow, onHistoryChanged, onModelDownloadProgress, openFolder, profilesAddPreset,
    profilesPresets, profilesTemplateStyle, setDefaultProfile, upsertProfile, type InstalledModel, type LastRun, type ManifestView, type PresetView, type Profile,
  } from "../../lib/api";
  import { appName, ensureAppNames } from "../../lib/apps.svelte";
  import { appliesDictionary, PUNCTUATION_CONFIGS, RULE_KEYS, rulesAt, SCRIPT_CONFIGS, SCRIPT_GROUPS, scriptAt, scriptLabel, selectedProfile, translateAt, enableTranslate, withDictionary, withRule, withRules, withScript, withScriptPunctuationBoth, withTranslate, type RuleKey, type TranslateStep } from "../../lib/flowEdit";
  import { CELL_IDS, cellLine, cellsOf, cellTime, moduleAdded, MODULE_IDS, runHeadline, stageName, type CellId } from "../../lib/flowLast";
  import { rememberProfile, rememberStage, storedProfile, storedStage } from "../../lib/stageMemory";
  import { guard } from "../../lib/guard.svelte";
  import { t } from "../../lib/i18n";
  import { isImeKey } from "../../lib/ime";
  import { appsSummary, freshName, withLlm } from "../../lib/profileEdit";
  import { anyFallback, FOLLOW, isFallback, resolveEngines, usableWith } from "../../lib/resolve";
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
  import InfoTip from "../../lib/ui/InfoTip.svelte";
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
  import StageRail, { type Cell, type Pick } from "./flow/StageRail.svelte";

  /** `arg` is the id of the profile to show; without it the default profile is shown. */
  let { arg }: { arg: string | null } = $props();

  const LANGS = ["zh-TW", "zh-CN", "en", "ja"];
  const LANG_KEYS: Record<string, string> = { "zh-TW": "profiles.lang.zh_tw", "zh-CN": "profiles.lang.zh_cn", en: "profiles.lang.en", ja: "profiles.lang.ja" };
  // The targets the translation module offers by name; anything else is typed in and kept as typed.
  const TARGET_KEYS = ["en", "ja", "zh_tw", "zh_cn", "ko", "fr", "de", "es"];
  const targetNames = $derived(TARGET_KEYS.map((k) => t(`flow.target.${k}`)));
  // The stage shown first is the one shown when the page was left last.
  // A stage, or "enable": the right column lists the modules not enabled.
  let stage = $state<Pick>(storedStage());
  $effect(() => { if (stage !== "enable") rememberStage(stage); });
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
  // The template a profile follows (the factory one when none is recorded), and whether its writing style has moved on since the profile last took it.
  const templateOf = (p: Profile) => presets.find((x) => x.id === (p.preset ?? "casual")) ?? null;
  const styleMoved = (p: Profile) => { const tpl = templateOf(p); return !!tpl && p.preset_revision < tpl.revision; };
  // The stored profiles with a writing style of their own whose template's style has moved on.
  const moved = $derived(new Set(profiles.filter(styleMoved).map((p) => p.id)));
  const movedTemplate = $derived(draft && styleMoved(draft) ? templateOf(draft) : null);
  // The first press of "take the new style" asks; the second replaces the profile's own text.
  let sureTake = $state(false);
  $effect(() => { source?.id; stage; sureTake = false; });
  async function settleStyle(take: boolean) {
    if (!source) return;
    busy = true;
    try {
      await profilesTemplateStyle(source.id, take);
      sureTake = false;
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      busy = false;
    }
  }
  const defaultName = (role: "asr" | "llm") => providers.find((p) => p.id === config.value?.default_engines[role])?.label ?? config.value?.default_engines[role] ?? "";
  const followChoice = (role: "asr" | "llm") => ({ name: t("flow.follow_default"), help: t("flow.follow_default_help", { name: defaultName(role) }) });
  // The engine the draft's template was tuned with; none for an untuned template or a profile without one.
  const suggested = $derived.by(() => {
    const preset = presets.find((p) => p.id === draft?.preset);
    return { asr: preset?.suggested_asr ?? null, llm: preset?.suggested_llm ?? null };
  });
  /** The fallback note of a stage: the engine the draft names cannot run, the default is used. */
  /**
   * Why a stage runs on the default engine instead of the one the profile names, and what fixes it:
   * the engine is gone, its model file is gone, a catalogue model is not downloaded, a speech model
   * lacks its audio encoder, or a service cannot run.
   */
  type Reason = "gone" | "file" | "download" | "encoder" | "service";
  function onDefault(role: "asr" | "llm" | "translate"): { wanted: string; using: string; reason: Reason; modelId: string | null; folder: string | null } | null {
    const c = role === "asr" ? resolved?.asr : role === "llm" ? resolved?.llm : resolved?.translate;
    if (!c || !isFallback(c)) return null;
    const wanted = providers.find((p) => p.id === c.source.wanted);
    const using = providers.find((p) => p.id === c.id)?.label ?? c.id;
    const modelId = wanted?.kind === "embedded" && wanted.model_id && manifest?.models.some((m) => m.id === wanted.model_id) ? wanted.model_id : null;
    const path = wanted?.kind === "embedded" ? wanted.model_path : null;
    const deaf = !!wanted && (installed.deaf?.has(wanted.id) || (!!path && installed.deaf?.has(path)));
    const reason: Reason = !wanted ? "gone" : deaf ? "encoder" : modelId ? "download" : path ? "file" : "service";
    return { wanted: wanted?.label ?? c.source.wanted, using, reason, modelId, folder: path ? path.replace(/[\\/][^\\/]*$/, "") : null };
  }
  // A download started from a stage's note, with its progress.
  let noteDownload = $state<{ id: string; received: number; total: number } | null>(null);
  async function downloadFromNote(id: string) {
    noteDownload = { id, received: 0, total: 0 };
    const off = await onModelDownloadProgress((p) => { if (p.id === id && !p.done) noteDownload = { id, received: p.received, total: p.total }; });
    try {
      await modelsDownload(id);
      models = await modelsInstalled();
      toast.ok(t("flow.downloaded"));
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      off();
      noteDownload = null;
    }
  }
  async function openModelFolder(folder: string) {
    try { await openFolder(folder); } catch (e) { toast.error(errorMessage(e)); }
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

  const lines = $derived.by((): Record<"mic" | "asr" | "llm" | "translate" | "out", StageLine> | null => {
    if (!draft) return null;
    return {
      mic: micLine(client.value?.microphone ?? "builtin", mics),
      asr: engineLine(providers, resolved?.asr.id ?? null, installed),
      llm: engineLine(providers, resolved?.llm?.id ?? null, installed),
      translate: engineLine(providers, resolved?.translate?.id ?? null, installed),
      out: outputLine(draft, client.value?.insertion.default),
    };
  });

  // What each stage did in the last dictation, computed when that run changes, not with every edit of the draft.
  const runLines = $derived.by((): Record<CellId, string> => {
    const run = last?.stages ?? null;
    return Object.fromEntries(CELL_IDS.map((id) => [id, cellLine(id, run)])) as Record<CellId, string>;
  });
  const runTimes = $derived.by((): Record<CellId, string | null> => {
    const run = last?.stages ?? null;
    return Object.fromEntries(CELL_IDS.map((id) => [id, cellTime(id, run)])) as Record<CellId, string | null>;
  });

  // One cell per stage the profile has: the stage's name, its current setting, and what it did in the last dictation.
  const cells = $derived.by((): Cell[] => {
    if (!draft || !lines) return [];
    const on = rulesAt(draft);
    const ruleCount = on ? RULE_KEYS.filter((k) => k !== "dictionary" && on[k]).length : 0;
    const rules = !on || (!on.dictionary && ruleCount === 0) ? t("flow.rules_none") : on.dictionary ? t("flow.rules_meta", { n: String(ruleCount) }) : t("flow.rules_meta_nodict", { n: String(ruleCount) });
    const translate = translateAt(draft);
    const cell = (id: CellId, setting: string, warn = false, unknown = false): Cell =>
      ({ id, name: stageName(id), setting, line: runLines[id], time: runTimes[id], warn, unknown, problem: problemOf(id) });
    const settings: Record<CellId, () => Cell> = {
      capture: () => cell("capture", lines!.mic.name, lines!.mic.missing),
      asr: () => cell("asr", lines!.asr.name, lines!.asr.missing, !lines!.asr.known),
      llm: () => cell("llm", lines!.llm.name, lines!.llm.missing, !lines!.llm.known),
      rules: () => cell("rules", rules),
      translate: () => cell("translate", translate ? translate.target : "", lines!.translate.missing, !lines!.translate.known),
      insert: () => cell("insert", lines!.out.name),
      record: () => cell("record", t("flow.record_meta")),
    };
    return cellsOf(draft).map((id) => settings[id]());
  });
  // The modules the draft has not enabled, listed in the right column from the row between the modules and the insertion.
  const addable = $derived.by((): { id: CellId; name: string; help: string }[] => {
    if (!draft) return [];
    return MODULE_IDS.filter((id) => !moduleAdded(draft!, id)).map((id) => ({ id, name: stageName(id), help: t(`flow.module.${id}_help`) }));
  });
  /** Enables a module at its fixed place and shows it; a module turned off earlier keeps its settings. A new translation starts with English as its target. */
  function addModule(id: CellId) {
    if (!draft) return;
    if (id === "llm") draft = withLlm(snapshot(), "");
    else if (id === "rules") draft = withRules(snapshot(), true);
    else if (id === "translate") draft = enableTranslate(snapshot(), t("flow.target.en"));
    stage = id;
  }
  /** The translation step with one field changed. */
  function setTranslate(change: Partial<TranslateStep>) {
    if (!draft) return;
    const step = translateAt(draft);
    if (step) draft = withTranslate(snapshot(), { ...step, ...change });
  }
  // The target picker: a named target, or "custom" with the typed text under it.
  const customTarget = $derived.by(() => { const s = draft ? translateAt(draft) : null; return !!s && !targetNames.includes(s.target); });
  // The disable row asks once more: a second press within a few seconds turns the module off.
  let sureRemove = $state(false);
  let sureTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => { stage; source?.id; clearTimeout(sureTimer); sureRemove = false; });
  function askRemove() {
    clearTimeout(sureTimer);
    sureRemove = true;
    sureTimer = setTimeout(() => { sureRemove = false; }, 4000);
  }
  /** Turns the shown module off in the draft, its settings kept, and shows the recognition stage. */
  function removeModule() {
    clearTimeout(sureTimer);
    sureRemove = false;
    if (!draft) return;
    if (stage === "llm") draft = withLlm(snapshot(), null);
    else if (stage === "rules") draft = withRules(snapshot(), false);
    else if (stage === "translate") draft = withTranslate(snapshot(), null);
    else return;
    stage = "asr";
  }
  // A module turned off in the stored profile elsewhere leaves the page on the recognition stage; so does the
  // module list once every module is enabled.
  $effect(() => { if (draft && (stage === "enable" ? addable.length === 0 : !cellsOf(draft).includes(stage))) stage = "asr"; });
  const current = $derived(stage === "enable" ? undefined : cells.find((c) => c.id === stage));
  /** The problem mark of an engine stage that runs on the default because the engine named for it cannot run, or of the rewrite stage whose template style has moved on. */
  function problemOf(id: CellId): string | undefined {
    const c = id === "asr" ? resolved?.asr : id === "llm" ? resolved?.llm : id === "translate" ? resolved?.translate : null;
    if (c && isFallback(c)) return t("flow.profile_fallback");
    return id === "llm" && movedTemplate ? t("flow.style_moved") : undefined;
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
      else if (isFallback(r.llm) || styleMoved(p)) stage = "llm";
    });
  });
  // The right column's title repeats the setting, and for an engine adds whether it is ready.
  const titleMeta = $derived.by(() => {
    if (!current || !lines) return "";
    const meta = stage === "asr" ? lines.asr.meta : stage === "llm" ? lines.llm.meta : stage === "translate" ? lines.translate.meta : stage === "capture" ? lines.mic.meta : "";
    if (!meta || meta === current.setting) return current.setting;
    return current.setting ? `${current.setting} · ${meta}` : meta;
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

<!-- What is wrong with the engine the stage names, that dictation goes on meanwhile, and a fix in place. -->
{#snippet fallbackNote(role: "asr" | "llm" | "translate")}
  {@const fb = onDefault(role)}
  {#if fb}
    <div class="fallback">
      <p><b>{t(`flow.reason.${fb.reason}`, { wanted: fb.wanted })}</b> {t("flow.fallback_then", { using: fb.using })}</p>
      <div class="acts">
        {#if fb.reason === "download" && fb.modelId}
          {#if noteDownload?.id === fb.modelId}
            <span class="sub num">{t("flow.downloading", { percent: String(noteDownload.total > 0 ? Math.floor((noteDownload.received / noteDownload.total) * 100) : 0) })}</span>
          {:else}
            <button type="button" class="btn" disabled={noteDownload !== null} onclick={() => downloadFromNote(fb.modelId!)}><Icon name="download" size={14} />{t("engines.download")}</button>
          {/if}
        {:else if (fb.reason === "file" || fb.reason === "encoder") && fb.folder}
          <button type="button" class="btn" onclick={() => openModelFolder(fb.folder!)}>{t("flow.open_model_folder")}</button>
        {:else if fb.reason === "service"}
          <button type="button" class="btn" onclick={() => guard.request(() => go("engines", role))}>{t("flow.to_engines")}</button>
        {/if}
        <button type="button" class="btn quiet" onclick={() => { if (role === "asr") draft!.asr_provider = null; else if (role === "llm") draft!.llm_provider = null; else setTranslate({ provider: null }); }}>{t("flow.use_default")}</button>
      </div>
    </div>
  {/if}
{/snippet}

<!-- The last row of a module's settings: removes it after a second press within a few seconds. -->
{#snippet removeRow(help: string)}
  <Row label={t("flow.disable_module")} {help}>
    {#if sureRemove}
      <button type="button" class="btn danger" onclick={removeModule}>{t("flow.disable_sure")}</button>
    {:else}
      <button type="button" class="btn quiet danger" onclick={askRemove}>{t("flow.disable")}</button>
    {/if}
  </Row>
{/snippet}

{#if config.value && draft && source && lines}
  <div class="pick">
    <ProfilePicker
      {profiles} current={source.id} name={draft.name.trim() || source.name} {applies} {faulty} {moved}
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
            <Chip onremove={() => draft!.matcher.app_ids.splice(i, 1)}><span use:tip={id}>{appName(id)}</span></Chip>
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
    <div class="railwrap"><StageRail {cells} selected={stage} onpick={(id) => (stage = id)} label={t("flow.stages")} entry={addable.length ? t("flow.enable_module") : ""} /></div>
    <div class="right">
      {#if current}
        <div class="ttl"><b>{current.name}</b><span class:warn={current.warn} class:unknown={current.unknown}>{titleMeta}</span></div>
      {/if}
      {#if stage !== "enable" && last !== undefined}
        <LastPanel id={stage} stages={last?.stages ?? null} row={last?.row ?? null} />
      {/if}

      {#if stage === "enable"}
        <div class="ttl"><b>{t("flow.enable_module")}</b></div>
        <Group help={t("flow.enable_help")}>
          {#each addable as m (m.id)}
            <Row label={m.name} help={m.help}>
              <button type="button" class="btn" onclick={() => addModule(m.id)}>{t("flow.enable")}</button>
            </Row>
          {/each}
        </Group>
      {:else if stage === "capture"}
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
          <Row label={t("flow.trim_meta")} help={t("flow.trim_help")} />
        </Group>
      {:else if stage === "asr"}
        <div class="bar"><h2>{t("flow.asr_engines")}</h2><button type="button" class="btn quiet" onclick={() => guard.request(() => go("engines", "asr"))}>{t("flow.manage")}</button></div>
        <!-- The reason for the stage's problem mark, above the engines to choose from. -->
        {@render fallbackNote("asr")}
        <EnginePicker
          engines={asrChoices} selected={draft.asr_provider ?? FOLLOW} label={t("flow.asr_engines")} {manifest} {installed} models={models ?? []} follow={followChoice("asr")} suggested={suggested.asr}
          defaultId={config.value?.default_engines.asr} onmanage={() => guard.request(() => go("engines", "asr"))}
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
          {@const script = scriptAt(draft, 1)}
          <Row label={t("flow.script")} help={t("flow.script_help")} info={t("flow.script_info")}>
            <!-- Sixteen fixed conversions: a list, grouped by the script they produce. A configuration set
                 outside this page is listed by its name and kept until another choice is made. The choice
                 covers the conversion after the rewrite too. -->
            <select class="field wide" value={script?.config ?? ""} aria-label={t("flow.script")} onchange={(e) => (draft = withScript(snapshot(), e.currentTarget.value || null))}>
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
              <Switch bind:checked={() => script.punctuation, (v) => (draft = withScriptPunctuationBoth(snapshot(), v))} label={t("flow.script_punct")} />
            </Row>
          </Fold>{/if}
          <Row label={t("flow.asr_hint")}>
            {#snippet help()}{t("flow.asr_hint_help_before").slice(0, -1)}<span class="nobr">{t("flow.asr_hint_help_before").slice(-1)}<button type="button" class="stage-link" onclick={() => (stage = "rules")}>{t("flow.stage.rules")}</button>{t("flow.asr_hint_help_after")}</span>{/snippet}
            <Switch bind:checked={draft.asr_dictionary_hint} label={t("flow.asr_hint")} />
          </Row>
          <Row label={t("flow.opencc_engine")}>
            {#snippet help()}{t("flow.opencc_engine_before")}<PageLink page="about" label={t("nav.about")} />{t("flow.opencc_engine_after")}{/snippet}
          </Row>
        </Group>
      {:else if stage === "llm"}
        <div class="bar"><h2>{t("flow.llm_engines")}</h2><button type="button" class="btn quiet" onclick={() => guard.request(() => go("engines", "llm"))}>{t("flow.manage")}</button></div>
        <!-- The reason for the stage's problem mark, above the engines to choose from. -->
        {@render fallbackNote("llm")}
        <EnginePicker
          engines={llmChoices} selected={draft.llm_provider ?? FOLLOW} label={t("flow.llm_engines")} {manifest} {installed} models={models ?? []}
          follow={followChoice("llm")} suggested={suggested.llm}
          defaultId={config.value?.default_engines.llm} onmanage={() => guard.request(() => go("engines", "llm"))}
          stale={stale(draft.llm_provider, llmChoices)} onpick={(id) => (draft = withLlm(snapshot(), id))}
        />
        {#if lines.llm.missing}
          <p class="missing"><span>{t("flow.model_missing")}</span><button type="button" class="btn" onclick={() => guard.request(() => go("engines", "llm"))}>{t("flow.to_download")}</button></p>
        {/if}
        <div class="bar"><h2>{t("profiles.prompt_addition")}</h2><button type="button" class="btn quiet" onclick={() => guard.request(() => go("prompt", "llm"))}>{t("flow.see_prompt")}</button></div>
          <!-- The reason for the stage's problem mark when the template's style moved on: the profile keeps its own text until the user decides. -->
          {#if movedTemplate}
            <div class="fallback">
              <p><b>{t("flow.style_moved_note", { preset: t(`profiles.preset.${movedTemplate.id}`) })}</b> {t("flow.style_moved_then")}</p>
              <div class="acts">
                <button type="button" class="btn quiet" disabled={busy} onclick={() => guard.request(() => settleStyle(false))}>{t("flow.style_keep")}</button>
                {#if sureTake}
                  <button type="button" class="btn primary" disabled={busy} onclick={() => guard.request(() => settleStyle(true))}>{t("flow.style_take_confirm")}</button>
                {:else}
                  <button type="button" class="btn" disabled={busy} onclick={() => (sureTake = true)}>{t("flow.style_take")}</button>
                {/if}
              </div>
            </div>
          {/if}
          <textarea rows="4" bind:value={draft.prompt_addition} aria-label={t("profiles.prompt_addition")}></textarea>
          <p class="help">{t("profiles.prompt_help")}<InfoTip info={t("profiles.prompt_info")} /></p>

          <Group title={t("profiles.few_shot")} help={t("profiles.few_shot_help")} info={t("profiles.few_shot_info")}>
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

          <Group title={t("flow.llm_rules")} info={t("flow.llm_rules_info")}>
            <Row label={t("flow.llm_terms")}>
              {#snippet help()}{t("flow.llm_terms_help_before").slice(0, -1)}<span class="nobr">{t("flow.llm_terms_help_before").slice(-1)}<button type="button" class="stage-link" onclick={() => (stage = "rules")}>{t("flow.stage.rules")}</button>{t("flow.llm_terms_help_after")}</span>{/snippet}
              <Switch bind:checked={draft.llm_dictionary_terms} label={t("flow.llm_terms")} />
            </Row>
            <Row label={t("flow.llm_title")} help={t("flow.llm_title_help")}>
              <Switch bind:checked={draft.llm_window_title} label={t("flow.llm_title")} />
            </Row>
            <Row label={t("flow.llm_short")} help={t("flow.llm_short_help")}>
              <Switch bind:checked={draft.llm_short_fallback} label={t("flow.llm_short")} />
            </Row>
          </Group>
        <Group>
          {@render removeRow(t("flow.disable_llm_help"))}
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
        <Group>
          {@render removeRow(t("flow.disable_rules_help"))}
        </Group>
      {:else if stage === "translate"}
        {@const step = translateAt(draft)!}
        <Group title={t("flow.translate_group")} info={t("flow.translate_info")}>
          <Row label={t("flow.translate_target")} help={t("flow.translate_target_help")}>
            <select class="field" value={customTarget ? "custom" : step.target} aria-label={t("flow.translate_target")} onchange={(e) => setTranslate({ target: e.currentTarget.value === "custom" ? "" : e.currentTarget.value })}>
              {#each targetNames as name (name)}<option value={name}>{name}</option>{/each}
              <option value="custom">{t("flow.translate_custom_option")}</option>
            </select>
          </Row>
          {#if customTarget}<Fold>
            <Row label={t("flow.translate_custom")} help={t("flow.translate_custom_help")}>
              <input type="text" class="field" value={step.target} aria-label={t("flow.translate_custom")} aria-invalid={step.target.trim() === "" ? "true" : undefined} oninput={(e) => setTranslate({ target: e.currentTarget.value })} />
            </Row>
          </Fold>{/if}
        </Group>
        <div class="bar"><h2>{t("flow.translate_addition")}</h2><button type="button" class="btn quiet" onclick={() => guard.request(() => go("prompt", "translate"))}>{t("flow.see_prompt")}</button></div>
        <textarea rows="4" value={step.addition ?? ""} aria-label={t("flow.translate_addition")} oninput={(e) => setTranslate({ addition: e.currentTarget.value })}></textarea>
        <p class="help">{t("flow.translate_addition_help")}<InfoTip info={t("profiles.prompt_info")} /></p>
        <div class="bar"><h2>{t("flow.translate_engines")}</h2><button type="button" class="btn quiet" onclick={() => guard.request(() => go("engines", "llm"))}>{t("flow.manage")}</button></div>
        {@render fallbackNote("translate")}
        <EnginePicker
          engines={llmChoices} selected={step.provider ?? FOLLOW} label={t("flow.translate_engines")} {manifest} {installed} models={models ?? []}
          follow={{ name: t("flow.follow_rewrite"), help: t("flow.follow_rewrite_help", { name: lines.llm.name === t("home.no_rewrite") ? defaultName("llm") : lines.llm.name }) }} suggested={null}
          defaultId={config.value?.default_engines.llm} onmanage={() => guard.request(() => go("engines", "llm"))}
          stale={stale(step.provider ?? null, llmChoices)} onpick={(id) => setTranslate({ provider: id === FOLLOW ? null : id })}
        />
        {#if lines.translate.missing}
          <p class="missing"><span>{t("flow.model_missing")}</span><button type="button" class="btn" onclick={() => guard.request(() => go("engines", "llm"))}>{t("flow.to_download")}</button></p>
        {/if}
        <Group>
          {@render removeRow(t("flow.disable_translate_help"))}
        </Group>
      {:else if stage === "insert"}
        <Group title={t("flow.stage.insert")}>
          <Row label={t("flow.insert_row")}>
            {#snippet help()}{t("flow.insert_help_before").slice(0, -1)}<span class="nobr">{t("flow.insert_help_before").slice(-1)}<PageLink page="recording" label={t("nav.recording")} />{t("flow.insert_help_after")}</span>{/snippet}
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
      <Row label={t("flow.add_default")} help={t(addDefault ? "flow.add_default_on" : "flow.add_default_off")} info={addDefault ? t("flow.add_default_on_info") : undefined}>
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
  /* Either column may be the taller one; each carries the page's bottom padding so the split ends with the content. */
  .split { display: grid; grid-template-columns: 236px minmax(0, 1fr); gap: 16px; align-items: start; margin-bottom: -80px; }
  .railwrap { position: sticky; top: 28px; padding-bottom: 80px; }
  .right { display: flex; flex-direction: column; min-width: 0; padding-bottom: 80px; }
  .right .ttl { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; margin: 2px 4px 10px; }
  .right .ttl b { font: 700 19px/1.3 var(--font-round); flex: none; }
  .right .ttl span { color: var(--ink-2); font-size: var(--text-sm); text-align: right; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .right .ttl span.warn { color: var(--warn); }
  .right .ttl span.unknown { color: var(--live); }
  /* The first group under the panel keeps the page's usual distance; the panel sits right under the title. */
  .right :global(.section:first-of-type h2) { margin-top: var(--gap-group); }

  .bar { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin: var(--gap-group) 4px 12px; min-height: 20px; }
  /* A stage named inside a help sentence, as a link that selects it on the rail. */
  .help :global(.info-btn) { margin-left: 2px; }
  .stage-link { border: 0; background: none; padding: 0; font: inherit; color: var(--accent); cursor: default; }
  .stage-link:hover { color: var(--ink); }
  .missing { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin: 10px 4px 0; color: var(--warn); font-size: var(--text-sm); }
  .fallback { margin: 0 0 12px; padding: 12px 14px; border-radius: var(--radius); background: color-mix(in srgb, var(--warn) 10%, var(--surface)); border: 1px solid color-mix(in srgb, var(--warn) 35%, transparent); font-size: var(--text-sm); }
  .fallback p { margin: 0; color: var(--ink); }
  .fallback b { color: var(--warn); font-weight: 600; }
  .fallback .acts { display: flex; align-items: center; justify-content: flex-end; gap: 8px; margin-top: 8px; }
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
