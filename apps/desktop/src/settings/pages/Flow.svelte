<script lang="ts">
  import { onMount, untrack } from "svelte";
  import {
    deleteProfile, errorMessage, historyLast, listMicrophones, modelsInstalled, modelsManifestNow, onHistoryChanged, profilesAddPreset,
    profilesPresets, setDefaultProfile, upsertProfile, type InstalledModel, type LastRun, type ManifestView, type PresetView, type Profile,
  } from "../../lib/api";
  import { appName, ensureAppNames } from "../../lib/apps.svelte";
  import { appliesDictionary, scriptChoice, selectedProfile, withDictionary, withScript } from "../../lib/flowEdit";
  import { cellLine, runHeadline, stageOpen, type StageId } from "../../lib/flowLast";
  import { guard } from "../../lib/guard.svelte";
  import { t } from "../../lib/i18n";
  import { isImeKey } from "../../lib/ime";
  import { appsSummary, blankProfile, slugId, withLlm } from "../../lib/profileEdit";
  import { go } from "../../lib/route";
  import { engineLine, micLine, outputLine, type StageLine } from "../../lib/stages";
  import { client, config } from "../../lib/store.svelte";
  import Chip from "../../lib/ui/Chip.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import SaveBar from "../../lib/ui/SaveBar.svelte";
  import Segmented from "../../lib/ui/Segmented.svelte";
  import Switch from "../../lib/ui/Switch.svelte";
  import { toast } from "../../lib/ui/toast.svelte";
  import AddPerApp from "./AddPerApp.svelte";
  import EnginePicker from "./flow/EnginePicker.svelte";
  import LastPanel from "./flow/LastPanel.svelte";
  import ProfilePicker from "./flow/ProfilePicker.svelte";
  import StageRail, { type Cell } from "./flow/StageRail.svelte";

  /** `arg` is the id of the profile to show; without it the default profile is shown. */
  let { arg }: { arg: string | null } = $props();

  const LANGS = ["zh-TW", "en", "ja"];
  const LANG_KEYS: Record<string, string> = { "zh-TW": "profiles.lang.zh_tw", en: "profiles.lang.en", ja: "profiles.lang.ja" };

  let stage = $state<StageId>("asr");
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
  let renaming = $state(false);
  let nameField: HTMLInputElement | undefined = $state();
  let confirmDelete = $state(false);
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
  const source = $derived(selectedProfile(config.value, arg));
  const providers = $derived(config.value?.providers ?? []);
  const asrChoices = $derived(providers.filter((p) => p.capabilities.transcribe));
  const llmChoices = $derived(providers.filter((p) => p.capabilities.chat));
  const installed = $derived({ ids: models ? new Set(models.map((m) => m.id)) : null, paths: new Set((models ?? []).map((m) => m.path)) });

  // Copy the stored profile into the draft: always for another profile, otherwise only while nothing is edited.
  $effect(() => {
    if (!source) { draft = null; return; }
    const next = comparable(source);
    const id = source.id;
    untrack(() => {
      if (draft === null || id !== syncedId || comparable(draft) === synced) {
        draft = structuredClone($state.snapshot(source)) as Profile;
        synced = next;
        if (id !== syncedId) { newApp = ""; newUrl = ""; newRaw = ""; newRefined = ""; renaming = false; }
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
      asr: engineLine(providers, draft.asr_provider, installed),
      llm: engineLine(providers, draft.llm_provider, installed),
      out: outputLine(draft, client.value?.insertion.default),
    };
  });

  // One cell per stage: the stage's name, its current setting, and what it did in the last dictation.
  const cells = $derived.by((): Cell[] => {
    if (!draft || !lines) return [];
    const run = last?.stages ?? null;
    const script = scriptChoice(draft);
    const scriptLabel = script === "tw" ? t("flow.script_tw") : script === "twp" ? t("flow.script_twp") : script === "none" ? t("flow.script_none") : script.other;
    const dict = config.value?.dictionary ?? [];
    const aliases = dict.reduce((n, e) => n + e.aliases.length, 0);
    const snippets = dict.filter((e) => e.term.includes("\n")).length;
    const rules = !appliesDictionary(draft) ? t("flow.rules_off") : dict.length === 0 ? t("flow.rules_meta_none") : t("flow.rules_meta", { aliases: String(aliases), snippets: String(snippets) });
    const cell = (id: StageId, name: string, setting: string, warn = false, unknown = false): Cell =>
      ({ id, name, setting, line: cellLine(id, run), open: stageOpen(id, draft!), warn, unknown });
    return [
      cell("capture", t("home.stage.capture"), lines.mic.name, lines.mic.missing),
      cell("trim", t("flow.stage.trim"), t("flow.trim_meta")),
      cell("asr", t("home.stage.asr"), lines.asr.name, lines.asr.missing, !lines.asr.known),
      cell("opencc1", t("flow.stage.opencc"), scriptLabel),
      cell("llm", t("home.stage.llm"), lines.llm.name, lines.llm.missing, !lines.llm.known),
      cell("opencc2", t("flow.stage.opencc"), stageOpen("opencc2", draft) ? t("flow.opencc_same") : scriptLabel),
      cell("rules", t("flow.stage.rules"), rules),
      cell("insert", t("flow.stage.insert"), lines.out.name),
      cell("record", t("flow.stage.record"), t("flow.record_meta")),
    ];
  });
  const current = $derived(cells.find((c) => c.id === stage));
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
      renaming = false;
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
    renaming = false;
  }

  async function makeDefault() {
    if (!source) return;
    try {
      config.value = await setDefaultProfile(source.id);
      if (draft) draft.is_default = true;
      toast.ok(t("profiles.made_default"));
    } catch (e) { toast.error(errorMessage(e)); }
  }
  async function remove() {
    if (!source) return;
    confirmDelete = false;
    saving = true;
    try {
      config.value = await deleteProfile(source.id);
      // The page is not asked about the edits of a profile that is gone.
      guard.set(false);
      toast.ok(t("profiles.deleted"));
      go("flow");
    } catch (e) { toast.error(errorMessage(e)); } finally { saving = false; }
  }

  async function startRename() {
    renaming = true;
    await Promise.resolve();
    nameField?.focus();
    nameField?.select();
  }

  /** Adds a profile (blank, or from a preset) and shows it. */
  async function create(presetId: string | null) {
    if (!config.value) return;
    busy = true;
    const before = new Set(config.value.profiles.map((p) => p.id));
    try {
      let next;
      if (presetId) {
        next = await profilesAddPreset(presetId, t(`profiles.preset.${presetId}`));
      } else {
        const base = config.value.profiles.find((p) => p.is_default) ?? config.value.profiles[0];
        const blank = blankProfile($state.snapshot(base) as Profile);
        blank.name = t("profiles.new_title");
        blank.id = slugId(blank.name, [...before]);
        next = await upsertProfile(blank);
      }
      config.value = next;
      const created = next.profiles.find((p) => !before.has(p.id));
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

{#if config.value && draft && source && lines}
  <div class="pick">
    <ProfilePicker {profiles} current={source.id} name={draft.name.trim() || source.name} {applies} onpick={(id) => guard.request(() => go("flow", id))} />
    <button type="button" class="btn add" onclick={() => guard.request(() => { revert(); adding = true; })}><Icon name="plus" size={14} />{t("flow.add")}</button>
  </div>

  <div class="who">
    {#if renaming}
      <input
        type="text" class="name-field" bind:this={nameField} bind:value={draft.name} aria-label={t("profiles.name")} placeholder={t("profiles.new_title")}
        aria-invalid={problem && !draft.name.trim() ? "true" : undefined}
        onkeydown={(e) => { if (e.key === "Enter" && !isImeKey(e)) renaming = false; else if (e.key === "Escape") { draft!.name = source.name; renaming = false; e.stopPropagation(); } }}
        onblur={() => { if (draft?.name.trim()) renaming = false; }}
      />
    {/if}
    <span class="where">{applies(draft)}</span>
    {#if !renaming}<button type="button" class="btn quiet" onclick={startRename}>{t("flow.rename")}</button>{/if}
    {#if !source.is_default}
      <button type="button" class="btn quiet" onclick={makeDefault} disabled={saving}>{t("profiles.make_default")}</button>
      <button type="button" class="btn quiet danger" onclick={() => (confirmDelete = true)} disabled={saving}>{t("profiles.delete")}</button>
    {/if}
  </div>

  <div class="gt">
    <span>{t("flow.stages")}</span>
    {#if last}<span class="r">{runHeadline(last.row, last.stages, new Date())}</span>{/if}
  </div>
  <div class="split">
    <StageRail {cells} selected={stage} onpick={(id) => (stage = id)} label={t("flow.stages")} />
    <div class="right">
      {#if current}
        <div class="ttl"><b>{current.name}</b><span class:warn={current.warn && current.open} class:unknown={current.unknown}>{titleMeta}</span></div>
      {/if}
      {#if last !== undefined}
        <LastPanel id={stage} stages={last?.stages ?? null} />
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
        <EnginePicker
          engines={asrChoices} selected={draft.asr_provider} label={t("flow.asr_engines")} {manifest} {installed}
          stale={stale(draft.asr_provider, asrChoices)} onpick={(id) => { if (id) draft!.asr_provider = id; }}
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
        <EnginePicker
          engines={llmChoices} selected={draft.llm_provider} label={t("flow.llm_engines")} {manifest} {installed}
          none={{ name: t("home.no_rewrite"), help: t("home.no_rewrite_meta") }}
          stale={stale(draft.llm_provider, llmChoices)} onpick={(id) => (draft = withLlm(snapshot(), id))}
        />
        {#if lines.llm.missing}
          <p class="missing"><span>{t("flow.model_missing")}</span><button type="button" class="btn" onclick={() => guard.request(() => go("engines", "llm"))}>{t("flow.to_download")}</button></p>
        {/if}
        {#if draft.llm_provider !== null}
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
        {/if}
      {:else if stage === "opencc1" || stage === "opencc2"}
        <Group title={t("flow.stage.opencc")} help={stage === "opencc2" ? t("flow.opencc_same") : undefined}>
          {@const script = scriptChoice(draft)}
          <Row label={t("flow.script")} help={t("flow.script_help")} />
          <!-- The choice sits under its label: four names side by side need the whole width of the column.
               A configuration set outside this page is shown by its name and kept until another choice is made. -->
          <div class="ctl">
            <Segmented
              options={[
                { value: "tw", label: t("flow.script_tw") },
                { value: "twp", label: t("flow.script_twp") },
                ...(typeof script === "object" ? [{ value: "other", label: script.other }] : []),
                { value: "none", label: t("flow.script_none") },
              ]}
              bind:value={() => (typeof script === "object" ? "other" : script), (v) => { if (v !== "other") draft = withScript(snapshot(), v); }}
              label={t("flow.script")}
            />
          </div>
        </Group>
      {:else if stage === "rules"}
        <Group title={t("flow.stage.rules")}>
          <Row label={t("flow.dictionary")} help={t("flow.dictionary_help")}>
            <Switch bind:checked={() => appliesDictionary(draft!), (v) => (draft = withDictionary(snapshot(), v))} label={t("flow.dictionary")} />
          </Row>
          <Row label={t("flow.rules_edit")} help={t("flow.rules_edit_help")} onclick={() => guard.request(() => go("dictionary"))} />
        </Group>
      {:else if stage === "insert"}
        <Group title={t("flow.stage.insert")}>
          <Row label={t("flow.insert_row")} help={t("flow.insert_help")}>
            <button type="button" class="btn quiet" onclick={() => guard.request(() => go("advanced"))}>{t("flow.to_advanced")}</button>
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
      {#if draft.matcher.url_patterns.length}
        <Row>
          <div class="chips">
            {#each draft.matcher.url_patterns as u, i (i)}<Chip onremove={() => draft!.matcher.url_patterns.splice(i, 1)}>{u}</Chip>{/each}
          </div>
        </Row>
      {/if}
    </Group>
  {/if}

  <SaveBar {dirty} {saving} message={problem} onsave={save} onrevert={revert} />
{/if}

<Modal bind:open={adding} title={t("flow.add")}>
  <div class="choices" class:busy inert={busy} aria-busy={busy}>
    <Row label={t("profiles.blank")} help={t("profiles.blank.desc")} onclick={() => create(null)} />
    {#each presets as p (p.id)}
      <Row label={t(`profiles.preset.${p.id}`)} help={t(`profiles.preset.${p.id}.desc`)} onclick={() => create(p.id)} />
    {/each}
  </div>
  {#snippet actions()}
    {#if busy}<span class="adding" role="status">{t("profiles.adding")}</span>{/if}
    <button type="button" class="btn" disabled={busy} onclick={() => (adding = false)}>{t("common.cancel")}</button>
  {/snippet}
</Modal>

<Modal bind:open={confirmDelete} title={t("profiles.delete_title")}>
  <p>{t("profiles.delete_body", { name: source?.name ?? "" })}</p>
  {#snippet actions()}
    <button type="button" class="btn" onclick={() => (confirmDelete = false)}>{t("common.cancel")}</button>
    <button type="button" class="btn danger" onclick={remove}>{t("profiles.delete")}</button>
  {/snippet}
</Modal>

<style>
  .pick { display: flex; align-items: center; gap: 10px; }
  .pick .add { height: 44px; border-radius: 12px; flex: none; }
  .who { display: flex; align-items: center; gap: 10px; min-height: var(--control); margin: 12px 4px 12px; }
  .name-field { width: 240px; flex: none; }
  .where { flex: 1; min-width: 0; color: var(--ink-2); font-size: var(--text-sm); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .who .btn:last-child { margin-right: -8px; }

  .gt { display: flex; justify-content: space-between; align-items: baseline; gap: 12px; margin: 22px 4px 12px; font: 600 var(--text-sm)/1 var(--font); color: var(--ink-2); }
  .gt .r { font-weight: 400; color: var(--ink-3); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .split { display: grid; grid-template-columns: 236px minmax(0, 1fr); gap: 16px; align-items: start; }
  .right { display: flex; flex-direction: column; min-width: 0; }
  .right .ttl { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; margin: 2px 4px 10px; }
  .right .ttl b { font: 700 19px/1.3 var(--font-round); flex: none; }
  .right .ttl span { color: var(--ink-2); font-size: var(--text-sm); text-align: right; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .right .ttl span.warn { color: var(--warn); }
  .right .ttl span.unknown { color: var(--live); }
  /* The first group under the panel keeps the page's usual distance; the panel sits right under the title. */
  .right :global(.section:first-of-type h2) { margin-top: var(--gap-group); }
  .ctl { padding: 0 var(--row-x) var(--row-y); margin-top: -6px; }

  .bar { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin: var(--gap-group) 4px 12px; min-height: 20px; }
  .missing { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin: 10px 4px 0; color: var(--warn); font-size: var(--text-sm); }
  .bar h2 { margin: 0; font: 600 var(--text-sm)/1 var(--font); color: var(--ink-2); }
  .bar .btn { margin: -10px -8px -10px 0; }
  .field { width: 240px; flex: none; }
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
  .choices.busy { opacity: .55; }
  .adding { margin-right: auto; align-self: center; color: var(--ink-2); font-size: var(--text-sm); }
  .choices { margin-top: 8px; border: 1px solid var(--line); border-radius: var(--radius-group); background: var(--surface); color: var(--ink); }
</style>
