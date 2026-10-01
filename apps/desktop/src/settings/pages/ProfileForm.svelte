<script lang="ts">
  import { tick, untrack } from "svelte";
  import { deleteProfile, errorMessage, setDefaultProfile, upsertProfile, type Profile, type StepSpec } from "../../lib/api";
  import { appName, ensureAppNames } from "../../lib/apps.svelte";
  import { guard } from "../../lib/guard.svelte";
  import { t } from "../../lib/i18n";
  import { isImeKey } from "../../lib/ime";
  import { defaultProfile } from "../../lib/labels";
  import { addStep as withStep, blankProfile, moveStep, NEW_PROFILE, removeStep, slugId, withLlm, type AddableStep } from "../../lib/profileEdit";
  import { config } from "../../lib/store.svelte";
  import Chip from "../../lib/ui/Chip.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import { toast } from "../../lib/ui/toast.svelte";
  import AddPerApp from "./AddPerApp.svelte";

  let { open = $bindable(true), arg }: {
    open?: boolean;
    /** A profile id, or NEW_PROFILE for a profile that is not saved yet. */
    arg: string;
  } = $props();

  const isNew = untrack(() => arg === NEW_PROFILE);
  const LANGS = ["zh-TW", "en", "ja"];
  const LANG_KEYS: Record<string, string> = { "zh-TW": "profiles.lang.zh_tw", en: "profiles.lang.en", ja: "profiles.lang.ja" };

  const source = $derived(isNew ? null : (config.value?.profiles.find((p) => p.id === arg) ?? null));
  const providers = $derived(config.value?.providers ?? []);
  const asrChoices = $derived(providers.filter((p) => p.capabilities.transcribe));
  const llmChoices = $derived(providers.filter((p) => p.capabilities.chat));

  let draft = $state<Profile | null>(null);
  // For a new profile: the draft as it was created.
  let initial = "";
  // For a saved profile: the stored profile the draft was last copied from.
  let synced = "";
  let saving = $state(false);
  let problem = $state<string | undefined>(undefined);
  // Deleting and closing with unsaved changes ask first, in place of the form.
  let view = $state<"form" | "delete" | "discard">("form");
  let safeButton: HTMLButtonElement | undefined = $state();
  // Typed but not added yet: counts as a change, and Save adds it first.
  let newApp = $state("");
  let newUrl = $state("");
  let newRaw = $state("");
  let newRefined = $state("");

  // Whether a profile is the default is changed on its own ("Make default"), not through the draft.
  const comparable = (p: Profile) => JSON.stringify({ ...p, is_default: false });

  if (isNew) {
    const base = untrack(() => defaultProfile(config.value));
    if (base) {
      const blank = blankProfile($state.snapshot(base) as Profile);
      draft = blank;
      initial = JSON.stringify(blank);
    }
  }

  // Copy the stored profile into the draft, and again when it changes while nothing is edited.
  $effect(() => {
    if (isNew || !source) return;
    const next = comparable(source);
    untrack(() => {
      if (draft === null || comparable(draft) === synced) {
        draft = structuredClone($state.snapshot(source)) as Profile;
        synced = next;
      }
    });
  });

  const pending = $derived(!!(newApp.trim() || newUrl.trim() || newRaw.trim() || newRefined.trim()));
  const halfExample = $derived(!newRaw.trim() !== !newRefined.trim());
  const dirty = $derived(
    draft !== null &&
      (pending || (isNew ? JSON.stringify(draft) !== initial : source !== null && comparable(draft) !== comparable(source))),
  );
  const missing = $derived(draft === null || (!isNew && source === null));
  const isDefault = $derived(source?.is_default ?? false);

  $effect(() => {
    guard.set(dirty);
    return () => guard.set(false);
  });
  $effect(() => {
    // A new edit clears the previous validation message.
    JSON.stringify([draft, newRaw, newRefined]);
    problem = undefined;
  });
  $effect(() => { if (draft) ensureAppNames(draft.matcher.app_ids); });

  /** Whether the stored engine exists and can do this job; "No rewriting" (null) is always usable. */
  function engineState(id: string | null, role: "transcribe" | "chat"): "ok" | "missing" | "unusable" {
    if (id === null) return "ok";
    const p = providers.find((x) => x.id === id);
    if (!p) return "missing";
    return p.capabilities[role] ? "ok" : "unusable";
  }

  const title = $derived(
    view === "delete" ? t("profiles.delete_title")
    : view === "discard" ? t("common.discard_title")
    : missing ? t("profiles.missing_title")
    : isNew ? t("profiles.add") : t("profiles.edit_title"),
  );

  async function ask(next: "delete" | "discard") {
    view = next;
    await tick();
    safeButton?.focus();
  }

  /** Esc or a click outside: a question goes back to the form, unsaved changes are asked about first. */
  function beforeclose(): boolean {
    if (view !== "form") { view = "form"; return false; }
    if (dirty) { ask("discard"); return false; }
    return true;
  }

  async function save() {
    if (!draft || !config.value) return;
    if (!draft.name.trim()) { problem = t("profiles.name_required"); return; }
    if (halfExample) { problem = t("profiles.example_incomplete"); return; }
    if (newApp.trim()) { addApp(newApp.trim()); newApp = ""; }
    addUrl();
    addExample();
    const next = $state.snapshot(draft) as Profile;
    if (isNew) next.id = slugId(next.name, config.value.profiles.map((p) => p.id));
    next.is_default = source?.is_default ?? false;
    saving = true;
    try {
      config.value = await upsertProfile(next);
      toast.ok(t("common.saved"));
      open = false;
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      saving = false;
    }
  }

  async function remove() {
    if (!source) return;
    saving = true;
    try {
      config.value = await deleteProfile(source.id);
      toast.ok(t("profiles.deleted"));
      open = false;
    } catch (e) {
      view = "form";
      toast.error(errorMessage(e));
    } finally {
      saving = false;
    }
  }

  async function makeDefault() {
    if (!source) return;
    try {
      config.value = await setDefaultProfile(source.id);
      toast.ok(t("profiles.made_default"));
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }

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
  // Steps and the rewriting engine change together: see addStep and removeStep.
  function addStep(select: HTMLSelectElement) {
    const kind = select.value as AddableStep | "";
    select.value = "";
    if (!draft || !kind) return;
    draft = withStep($state.snapshot(draft) as Profile, kind, llmChoices.map((p) => p.id));
  }
  function dropStep(i: number) {
    if (draft) draft = removeStep($state.snapshot(draft) as Profile, i);
  }
  function move(i: number, delta: number) {
    if (draft) draft.steps = moveStep($state.snapshot(draft.steps) as StepSpec[], i, delta);
  }
  function stepLabel(s: StepSpec): string {
    if (s.step === "opencc") return s.config === "s2twp" ? t("profiles.step.opencc") : t("profiles.step.opencc_config", { config: s.config });
    return t(`profiles.step.${s.step}`);
  }
  function setLlm(value: string) {
    if (draft) draft = withLlm($state.snapshot(draft) as Profile, value === "" ? null : value);
  }
</script>

{#snippet unavailable(id: string, state: "missing" | "unusable")}
  {#if state === "missing"}<option value={id} disabled>{t("profiles.not_found_option", { id })}</option>
  {:else}<option value={id} disabled>{t("profiles.unusable_option", { name: providers.find((p) => p.id === id)?.label ?? id })}</option>{/if}
{/snippet}

<Modal bind:open {title} size={view === "form" && !missing ? "lg" : "md"} {beforeclose}>
  {#if view === "delete"}
    <p>{t("profiles.delete_body", { name: source?.name ?? "" })}</p>
  {:else if view === "discard"}
    <p>{t("profiles.discard_body")}</p>
  {:else if missing}
    <p>{t("profiles.missing")}</p>
  {/if}

  {#if draft && !missing}
    <!-- Kept in place while a question is shown, so what was typed is still there afterwards. -->
    <div class="form" hidden={view !== "form"}>
      <p class="lead">{t(isDefault ? "profiles.default_desc" : "profiles.detail_desc")}</p>

      <Group title={t("profiles.basics")}>
        <Row label={t("profiles.name")}>
          <input type="text" class="field" bind:value={draft.name} aria-label={t("profiles.name")} placeholder={t("profiles.new_title")} aria-invalid={problem && !draft.name.trim() ? "true" : undefined} />
        </Row>
        {#if source}
          <Row label={t("profiles.default_label")} help={t(isDefault ? "profiles.is_default" : "profiles.not_default")}>
            {#if isDefault}<Tag tone="accent">{t("profiles.default")}</Tag>
            {:else}<button type="button" class="btn" onclick={makeDefault}>{t("profiles.make_default")}</button>{/if}
          </Row>
        {/if}
      </Group>

      {#if !isDefault}
        <Group title={t("profiles.applies")}>
          <Row label={t("profiles.apps_label")} />
          <Row>
            <div class="chips">
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

      <Group title={t("profiles.engines")}>
        {@const asrState = engineState(draft.asr_provider, "transcribe")}
        {@const llmState = engineState(draft.llm_provider, "chat")}
        <Row label={t("profiles.asr")}>
          <select class="field" class:unknown={asrState !== "ok"} bind:value={draft.asr_provider} aria-label={t("profiles.asr")}>
            {#each asrChoices as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
            {#if asrState !== "ok"}{@render unavailable(draft.asr_provider, asrState)}{/if}
          </select>
        </Row>
        <Row label={t("profiles.llm")}>
          <select class="field" class:unknown={llmState !== "ok"} value={draft.llm_provider ?? ""} aria-label={t("profiles.llm")} onchange={(e) => setLlm(e.currentTarget.value)}>
            {#each llmChoices as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
            <option value="">{t("home.no_rewrite")}</option>
            {#if draft.llm_provider !== null && llmState !== "ok"}{@render unavailable(draft.llm_provider, llmState)}{/if}
          </select>
        </Row>
        <Row label={t("profiles.language_hint")} help={t("profiles.language_help")}>
          <select class="field" bind:value={() => draft!.language_hint ?? "", (v) => (draft!.language_hint = v || null)} aria-label={t("profiles.language_hint")}>
            {#each LANGS as l (l)}<option value={l}>{t(LANG_KEYS[l])}</option>{/each}
            {#if draft.language_hint && !LANGS.includes(draft.language_hint)}<option value={draft.language_hint}>{draft.language_hint}</option>{/if}
            <option value="">{t("profiles.lang.auto")}</option>
          </select>
        </Row>
      </Group>

      <h3 class="title">{t("profiles.prompt_addition")}</h3>
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

      <Group title={t("profiles.steps")}>
        {#each draft.steps as s, i (i)}
          <Row>
            <span class="num idx">{i + 1}</span>
            <span class="step">{stepLabel(s)}</span>
            {#if s.step !== "asr"}
              <span class="tools">
                <button type="button" class="btn quiet icon-btn" aria-label={t("profiles.move_up")} disabled={i === 0 || draft.steps[i - 1].step === "asr"} onclick={() => move(i, -1)}><Icon name="arrow-up" /></button>
                <button type="button" class="btn quiet icon-btn" aria-label={t("profiles.move_down")} disabled={i === draft.steps.length - 1} onclick={() => move(i, 1)}><Icon name="arrow-down" /></button>
                <button type="button" class="btn quiet icon-btn" aria-label={t("common.remove")} onclick={() => dropStep(i)}><Icon name="trash" /></button>
              </span>
            {/if}
          </Row>
        {/each}
        <Row>
          <span class="menu">
            <span class="btn quiet" aria-hidden="true"><Icon name="plus" size={14} />{t("profiles.add_step")}</span>
            <select aria-label={t("profiles.add_step")} value="" onchange={(e) => addStep(e.currentTarget)}>
              <option value="" disabled hidden>{t("profiles.add_step")}</option>
              <option value="opencc">{t("profiles.step.opencc")}</option>
              {#if llmChoices.length}<option value="llm">{t("profiles.step.llm")}</option>
              {:else}<option value="llm" disabled>{t("profiles.step.llm_unavailable")}</option>{/if}
              <option value="rules">{t("profiles.step.rules")}</option>
            </select>
          </span>
        </Row>
      </Group>
    </div>
  {/if}

  {#snippet actions()}
    {#if view === "delete"}
      <button type="button" class="btn" bind:this={safeButton} onclick={() => (view = "form")}>{t("common.cancel")}</button>
      <button type="button" class="btn danger" onclick={remove} disabled={saving}>{t("profiles.delete")}</button>
    {:else if view === "discard"}
      <button type="button" class="btn" bind:this={safeButton} onclick={() => (view = "form")}>{t("common.stay")}</button>
      <button type="button" class="btn danger" onclick={() => (open = false)}>{t("common.discard")}</button>
    {:else if missing}
      <button type="button" class="btn" onclick={() => (open = false)}>{t("common.dismiss")}</button>
    {:else}
      {#if source && !isDefault}<button type="button" class="btn quiet danger delete" onclick={() => ask("delete")} disabled={saving}>{t("profiles.delete")}</button>{/if}
      {#if problem}<span class="problem" role="alert">{problem}</span>{/if}
      <button type="button" class="btn" onclick={() => (open = false)}>{t("common.cancel")}</button>
      <button type="button" class="btn primary" onclick={save} disabled={saving || !(dirty || isNew)}>{t("common.save")}</button>
    {/if}
  {/snippet}
</Modal>

<style>
  .lead { margin: 14px 2px 0; color: var(--ink-2); }
  .field { width: 220px; flex: none; }
  /* The stored engine no longer exists; the select shows its id. */
  select.unknown { border-color: var(--warn); color: var(--warn); }
  select.unknown option { color: var(--ink); }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; flex: 1; min-width: 0; }
  .title { font: 600 12px/1 var(--font); color: var(--ink-2); margin: 22px 2px 8px; }
  textarea { display: block; }
  .pair-text { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .arrow { flex: none; color: var(--ink-3); }
  /* Keeps the arrows of the example rows and the add row in one column. */
  .end { width: 76px; flex: none; display: flex; justify-content: flex-end; }
  .end .icon-btn { margin-right: -6px; }
  .icon-btn { width: 26px; padding: 0; color: var(--ink-2); }
  .icon-btn:hover { color: var(--ink); }
  .idx { width: 16px; flex: none; color: var(--ink-2); font-size: 12px; text-align: right; }
  .step { flex: 1; min-width: 0; }
  .tools { display: flex; gap: 2px; margin-right: -6px; }
  /* A native menu: the select is invisible and covers the quiet button. */
  .menu { position: relative; display: inline-flex; margin-left: -6px; }
  .menu select { position: absolute; inset: 0; width: 100%; height: 100%; opacity: 0; padding: 0; border: 0; cursor: default; }
  .menu:hover .btn { background: var(--accent-soft); }
  .menu:has(select:focus-visible) .btn { outline: 2px solid var(--accent); outline-offset: 1px; }
  .delete { margin-right: auto; margin-left: -6px; }
  .problem { flex: 1; min-width: 0; align-self: center; color: var(--live); font-size: 12px; }
</style>
