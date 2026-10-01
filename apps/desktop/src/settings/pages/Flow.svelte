<script lang="ts">
  import { onMount, untrack } from "svelte";
  import {
    deleteProfile, errorMessage, listMicrophones, modelsInstalled, modelsManifestNow, profilesAddPreset, profilesPresets,
    setDefaultProfile, upsertProfile, type InstalledModel, type ManifestView, type PresetView, type Profile,
  } from "../../lib/api";
  import { appName, ensureAppNames } from "../../lib/apps.svelte";
  import { appliesDictionary, scriptChoice, selectedProfile, withDictionary, withScript } from "../../lib/flowEdit";
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
  import Tag from "../../lib/ui/Tag.svelte";
  import { toast } from "../../lib/ui/toast.svelte";
  import AddPerApp from "./AddPerApp.svelte";
  import EnginePicker from "./flow/EnginePicker.svelte";

  /** `arg` is the id of the profile to show; without it the default profile is shown. */
  let { arg }: { arg: string | null } = $props();

  type Stage = "mic" | "asr" | "llm" | "out";
  const STAGES: { id: Stage; key: string }[] = [
    { id: "mic", key: "home.stage.capture" }, { id: "asr", key: "home.stage.asr" },
    { id: "llm", key: "home.stage.llm" }, { id: "out", key: "home.stage.output" },
  ];
  const LANGS = ["zh-TW", "en", "ja"];
  const LANG_KEYS: Record<string, string> = { "zh-TW": "profiles.lang.zh_tw", en: "profiles.lang.en", ja: "profiles.lang.ja" };

  let stage = $state<Stage>("asr");
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
    listMicrophones().then((m) => { mics = m; }).catch(() => { mics = []; });
    modelsInstalled().then((m) => { models = m; }).catch(() => { models = null; });
    modelsManifestNow().then((m) => { manifest = m; }).catch(() => { manifest = null; });
    profilesPresets().then((p) => { presets = p; }).catch((e) => toast.error(t("profiles.presets_failed", { detail: errorMessage(e) })));
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

  const lines = $derived.by((): Record<Stage, StageLine> | null => {
    if (!draft) return null;
    return {
      mic: micLine(client.value?.microphone ?? "builtin", mics),
      asr: engineLine(providers, draft.asr_provider, installed),
      llm: engineLine(providers, draft.llm_provider, installed),
      out: outputLine(draft),
    };
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
      toast.ok(t("common.saved"));
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
  <div class="tabs" role="tablist" aria-label={t("flow.profiles")}>
    {#each profiles as p (p.id)}
      <button type="button" class="tab" class:on={p.id === source.id} role="tab" aria-selected={p.id === source.id} onclick={() => guard.request(() => go("flow", p.id))}>{p.name}</button>
    {/each}
    <button type="button" class="tab add" onclick={() => guard.request(() => { revert(); adding = true; })}><Icon name="plus" size={14} />{t("flow.add")}</button>
  </div>

  <div class="who">
    {#if renaming}
      <input
        type="text" class="name-field" bind:this={nameField} bind:value={draft.name} aria-label={t("profiles.name")} placeholder={t("profiles.new_title")}
        aria-invalid={problem && !draft.name.trim() ? "true" : undefined}
        onkeydown={(e) => { if (e.key === "Enter" && !isImeKey(e)) renaming = false; else if (e.key === "Escape") { draft!.name = source.name; renaming = false; e.stopPropagation(); } }}
        onblur={() => { if (draft?.name.trim()) renaming = false; }}
      />
    {:else}
      <span class="name">{draft.name}</span>
    {/if}
    {#if source.is_default}<Tag tone="accent">{t("profiles.default")}</Tag>{/if}
    <span class="where">{applies(draft)}</span>
    {#if !renaming}<button type="button" class="btn quiet" onclick={startRename}>{t("flow.rename")}</button>{/if}
    {#if !source.is_default}
      <button type="button" class="btn quiet" onclick={makeDefault} disabled={saving}>{t("profiles.make_default")}</button>
      <button type="button" class="btn quiet danger" onclick={() => (confirmDelete = true)} disabled={saving}>{t("profiles.delete")}</button>
    {/if}
  </div>

  <div class="flow" role="tablist" aria-label={t("flow.stages")}>
    {#each STAGES as s, i (s.id)}
      {@const line = lines[s.id]}
      <button type="button" class="stage" class:on={stage === s.id} role="tab" aria-selected={stage === s.id} onclick={() => (stage = s.id)}>
        <span class="role"><b>{i + 1}</b>{t(s.key)}</span>
        <span class="what" class:unknown={!line.known} title={line.name}>{line.name}</span>
        <span class="meta" class:warn={line.missing || !line.known}>{line.meta}</span>
      </button>
    {/each}
  </div>

  {#if stage === "mic"}
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
    {/if}
  {:else}
    <Group title={t("home.stage.output")}>
      <Row label={t("flow.script")} help={t("flow.script_help")}>
        {@const script = scriptChoice(draft)}
        <!-- A configuration set outside this page is shown by its name and kept until another choice is made. -->
        <Segmented
          options={[
            { value: "tw", label: t("flow.script_tw") },
            ...(typeof script === "object" ? [{ value: "other", label: script.other }] : []),
            { value: "none", label: t("flow.script_none") },
          ]}
          bind:value={() => (typeof script === "object" ? "other" : script), (v) => { if (v !== "other") draft = withScript(snapshot(), v === "tw", true); }}
          label={t("flow.script")}
        />
      </Row>
      <Row label={t("flow.dictionary")} help={t("flow.dictionary_help")}>
        <Switch bind:checked={() => appliesDictionary(draft!), (v) => (draft = withDictionary(snapshot(), v))} label={t("flow.dictionary")} />
      </Row>
      <Row label={t("flow.insert_row")} help={t("flow.insert_help")}>
        <button type="button" class="btn quiet" onclick={() => guard.request(() => go("general"))}>{t("flow.to_general")}</button>
      </Row>
    </Group>
  {/if}

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
  .tabs { display: flex; flex-wrap: wrap; gap: 8px; }
  .tab { display: inline-block; height: 40px; max-width: 260px; padding: 0 18px; border-radius: 20px; border: 1px solid var(--line-strong); background: var(--surface); font-size: var(--text-control); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; cursor: default; }
  .tab:hover { background: var(--sunken); }
  .tab.on { background: var(--accent); border-color: transparent; color: var(--accent-ink); font-weight: 600; }
  .tab.add { display: inline-flex; align-items: center; gap: 7px; border-style: dashed; color: var(--accent); }
  .who { display: flex; align-items: center; gap: 10px; min-height: var(--control); margin: 24px 4px 12px; }
  .name { font-size: var(--text-lg); font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 40%; flex: none; }
  .name-field { width: 240px; flex: none; }
  .where { flex: 1; min-width: 0; color: var(--ink-2); font-size: var(--text-sm); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .who .btn:last-child { margin-right: -8px; }

  .flow { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); overflow: hidden; }
  .stage { position: relative; display: flex; flex-direction: column; align-items: stretch; padding: 20px 22px 24px; border: 0; border-right: 1px solid var(--line); background: none; text-align: left; cursor: default; min-width: 0; }
  .stage:last-child { border-right: 0; }
  .stage:hover { background: var(--sunken); }
  .stage.on { background: var(--accent-soft); }
  .stage.on::after { content: ""; position: absolute; left: 22px; right: 22px; bottom: 0; height: 3px; border-radius: 3px 3px 0 0; background: var(--accent); }
  .stage:focus-visible { outline-offset: -2px; }
  /* The notch that points from one stage to the next. */
  .stage + .stage::before { content: ""; position: absolute; left: -6px; top: 25px; width: 10px; height: 10px; background: var(--surface); border-top: 1px solid var(--line-strong); border-right: 1px solid var(--line-strong); transform: rotate(45deg); z-index: 1; }
  .role { display: flex; align-items: center; gap: 8px; font-size: var(--text-xs); color: var(--ink-2); }
  .role b { display: inline-grid; place-items: center; width: 20px; height: 20px; border-radius: 50%; background: var(--line); font-size: var(--text-xs); font-weight: 600; }
  .stage.on .role b { background: var(--accent); color: var(--accent-ink); }
  .what { margin-top: 12px; font: 600 var(--text-lg)/1.3 var(--font-round); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .what.unknown { color: var(--warn); }
  .meta { margin-top: 3px; color: var(--ink-2); font-size: var(--text-sm); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta.warn { color: var(--warn); }

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
