<script lang="ts">
  import { tick, untrack } from "svelte";
  import { errorMessage, profilesAddPreset, setDefaultProfile, upsertProfile, type InstalledModel, type ManifestView, type PresetView, type ProviderConfig } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import { freshName, withEngines } from "../../../lib/profileEdit";
  import { FOLLOW } from "../../../lib/resolve";
  import { go } from "../../../lib/route";
  import { engineKind, type Installed } from "../../../lib/stages";
  import { config } from "../../../lib/store.svelte";
  import Icon from "../../../lib/ui/Icon.svelte";
  import Modal from "../../../lib/ui/Modal.svelte";
  import Row from "../../../lib/ui/Row.svelte";
  import Steps from "../../../lib/ui/Steps.svelte";
  import Switch from "../../../lib/ui/Switch.svelte";
  import { toast } from "../../../lib/ui/toast.svelte";
  import EnginePicker from "./EnginePicker.svelte";

  /** Adds a profile in three steps: its template, the engines it runs on, its name. */
  let { open = $bindable(false), presets, manifest, installed, models }: {
    open?: boolean;
    presets: PresetView[];
    manifest: ManifestView | null;
    installed: Installed;
    models: InstalledModel[];
  } = $props();

  type Step = "template" | "engines" | "name";
  type Role = "asr" | "llm";
  const ORDER: Step[] = ["template", "engines", "name"];

  let step = $state<Step>("template");
  let preset = $state("");
  let name = $state("");
  // Set once the name is typed; until then the name follows the chosen template.
  let renamed = $state(false);
  let makeDefault = $state(false);
  // Undefined until the user picks: the engine follows the chosen template.
  let pickedAsr = $state<string | undefined>(undefined);
  let pickedLlm = $state<string | null | undefined>(undefined);
  let busy = $state(false);
  let confirmDiscard = $state(false);
  let keepButton: HTMLButtonElement | undefined = $state();

  const template = $derived(presets.find((p) => p.id === preset));
  const templateEngines = $derived({ asr: template?.suggested_asr ?? FOLLOW, llm: template?.suggested_llm ?? FOLLOW });
  const asr = $derived(pickedAsr ?? templateEngines.asr);
  const llm = $derived(pickedLlm === undefined ? templateEngines.llm : pickedLlm);
  const presetName = (id: string) => t(`profiles.preset.${id}`);
  const takenNames = () => (config.value?.profiles ?? []).map((p) => p.name);
  const stepAt = $derived(ORDER.indexOf(step));
  const changed = $derived(stepAt > 0 || renamed || preset !== (presets[0]?.id ?? ""));

  $effect.pre(() => {
    if (!open) return;
    untrack(() => {
      step = "template";
      preset = presets[0]?.id ?? "";
      name = preset ? freshName(presetName(preset), takenNames()) : "";
      renamed = false;
      makeDefault = false;
      pickedAsr = undefined;
      pickedLlm = undefined;
      confirmDiscard = false;
    });
  });

  function beforeclose(): boolean {
    if (busy) return false;
    // Esc on the question answers no; the wizard stays as it was.
    if (confirmDiscard) { confirmDiscard = false; return false; }
    if (!changed) return true;
    confirmDiscard = true;
    tick().then(() => keepButton?.focus());
    return false;
  }

  function choosePreset(id: string) {
    preset = id;
    if (!renamed) name = freshName(presetName(id), takenNames());
  }

  /** The engines of a role, with the template's: the model a template names gets its engine when the profile is added. */
  function choices(role: Role): ProviderConfig[] {
    const all = config.value?.providers ?? [];
    const own = all.filter((p) => (role === "asr" ? p.capabilities.transcribe : p.capabilities.chat));
    const id = templateEngines[role];
    const entry = manifest?.models.find((m) => m.id === id);
    if (!id || !entry || own.some((p) => p.id === id)) return own;
    return [...own, {
      id, label: entry.label, kind: "embedded", capabilities: { transcribe: role === "asr", chat: role === "llm", streaming: false, language_hint: false, hotwords: false },
      base_url: null, model: null, api_key_ref: null, extra_headers: {}, model_path: null, native_id: null, model_id: id, runtime: entry.runtime,
    }];
  }
  const defaultId = (role: Role) => config.value?.default_engines[role] ?? "";
  const defaultName = (role: Role) => (config.value?.providers ?? []).find((p) => p.id === defaultId(role))?.label ?? defaultId(role);
  const follow = (role: Role) => ({ name: t("flow.follow_default"), help: t("flow.follow_default_help", { name: defaultName(role) }) });
  /** Whether the engine a stage would run on is a model still to download. */
  function undownloaded(role: Role, picked: string | null): boolean {
    if (picked === null) return false;
    const id = picked === FOLLOW ? defaultId(role) : picked;
    const engine = (config.value?.providers ?? []).find((p) => p.id === id) ?? choices(role).find((p) => p.id === id);
    return !!engine && engineKind(engine, installed).reason === "download";
  }

  /** Adds a copy of the chosen template with the picked engines, makes it the default when asked, and shows it. */
  async function create() {
    if (!config.value || !preset || !name.trim()) return;
    busy = true;
    const before = new Set(config.value.profiles.map((p) => p.id));
    try {
      let next = await profilesAddPreset(preset, name.trim());
      const created = next.profiles.find((p) => !before.has(p.id));
      if (created && (asr !== templateEngines.asr || llm !== templateEngines.llm)) next = await upsertProfile(withEngines(created, asr, llm));
      if (created && makeDefault) next = await setDefaultProfile(created.id);
      config.value = next;
      open = false;
      toast.ok(t("common.saved"));
      if (created) go("flow", created.id);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      busy = false;
    }
  }
</script>

<Modal bind:open title={confirmDiscard ? t("common.discard_title") : t("flow.add")} {beforeclose}>
  {#if confirmDiscard}
    <p>{t("flow.add_discard_body")}</p>
  {:else}
  <div class="wizard" class:busy inert={busy} aria-busy={busy}>
    <Steps current={step} steps={[{ id: "template", label: t("flow.add_template") }, { id: "engines", label: t("flow.add_engines") }, { id: "name", label: t("flow.add_name") }]} onpick={(id) => (step = id as Step)} />
    {#if step === "template"}
      <div class="templates" role="radiogroup" aria-label={t("flow.add_template")}>
        {#each presets as p (p.id)}
          <label class="template" class:on={preset === p.id} class:blank={p.id === "blank"}>
            <input type="radio" name="preset" value={p.id} checked={preset === p.id} onchange={() => choosePreset(p.id)} />
            <span><b>{presetName(p.id)}</b><small>{t(`profiles.preset.${p.id}.desc`)}</small></span>
          </label>
        {/each}
      </div>
    {:else if step === "engines"}
      <div class="engines">
        <div>
          <h3>{t("flow.asr_engines")}</h3>
          <EnginePicker
            engines={choices("asr")} selected={asr} label={t("flow.asr_engines")} {manifest} {installed} {models} follow={follow("asr")}
            suggested={template?.suggested_asr ?? null} defaultId={defaultId("asr")} onpick={(id) => (pickedAsr = id ?? FOLLOW)}
          />
          {#if undownloaded("asr", asr)}<p class="note warn">{t("flow.add_download_later")}</p>{/if}
        </div>
        <div>
          <h3>{t("flow.llm_engines")}</h3>
          <EnginePicker
            engines={choices("llm")} selected={llm} label={t("flow.llm_engines")} {manifest} {installed} {models} follow={follow("llm")}
            none={{ name: t("home.no_rewrite"), help: t("flow.disable_llm_help") }}
            suggested={template?.suggested_llm ?? null} defaultId={defaultId("llm")} onpick={(id) => (pickedLlm = id)}
          />
          <!-- Every template has the rewrite stage, so turning it off here always differs from the template. -->
          {#if llm === null}<p class="note">{t("flow.add_no_rewrite", { preset: presetName(preset) })}</p>{/if}
          {#if undownloaded("llm", llm)}<p class="note warn">{t("flow.add_download_later")}</p>{/if}
        </div>
      </div>
    {:else}
      <div class="choices">
        <Row label={t("flow.add_name")}>
          <input class="field" type="text" bind:value={name} oninput={() => (renamed = true)} aria-label={t("flow.add_name")} />
        </Row>
        <Row label={t("flow.add_default")} help={t(makeDefault ? "flow.add_default_on" : "flow.add_default_off")} info={makeDefault ? t("flow.add_default_on_info") : undefined}>
          <Switch bind:checked={makeDefault} label={t("flow.add_default")} />
        </Row>
      </div>
    {/if}
  </div>
  {/if}
  {#snippet actions()}
    {#if confirmDiscard}
      <button type="button" class="btn" bind:this={keepButton} onclick={() => (confirmDiscard = false)}>{t("common.cancel")}</button>
      <button type="button" class="btn danger" onclick={() => (open = false)}>{t("common.discard")}</button>
    {:else}
    {#if busy}<span class="adding" role="status">{t("profiles.adding")}</span>
    {:else if stepAt > 0}<button type="button" class="btn quiet lead" onclick={() => (step = ORDER[stepAt - 1])}><Icon name="chevron-left" size={14} />{t("engines.back")}</button>{/if}
    <button type="button" class="btn" disabled={busy} onclick={() => (open = false)}>{t("common.cancel")}</button>
    {#if step === "name"}
      <button type="button" class="btn primary" disabled={busy || !preset || !name.trim()} onclick={create}>{t("flow.add_create")}</button>
    {:else}
      <button type="button" class="btn primary" disabled={!preset} onclick={() => (step = ORDER[stepAt + 1])}>{t("flow.add_next")}</button>
    {/if}
    {/if}
  {/snippet}
</Modal>

<style>
  .wizard { display: flex; flex-direction: column; gap: 14px; padding-top: 2px; }
  .busy { opacity: .55; }
  .templates { display: flex; flex-direction: column; gap: 8px; }
  .template { display: flex; align-items: flex-start; gap: 12px; padding: 11px 14px; border: 1px solid var(--line); border-radius: var(--radius); background: var(--surface); color: var(--ink); cursor: default; }
  .template.on { border-color: var(--accent); background: var(--accent-soft); }
  .template.blank { margin-top: 10px; border-style: dashed; border-color: var(--line-strong); background: none; }
  .template.blank.on { border-style: solid; border-color: var(--accent); background: var(--accent-soft); }
  .template input { appearance: none; flex: none; width: 16px; height: 16px; margin: 4px 0 0; border: 1.5px solid var(--ink-3); border-radius: 50%; }
  .template input:checked { border: 5px solid var(--accent); }
  .template input:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .template b { display: block; font-weight: 600; }
  .template small { display: block; color: var(--ink-2); font-size: var(--text-sm); }
  .engines { display: flex; flex-direction: column; gap: 16px; }
  h3 { margin: 0 4px 8px; font: 600 var(--text-sm)/1 var(--font); color: var(--ink-2); }
  .note { margin: 4px 4px 0; color: var(--ink-2); font-size: var(--text-sm); }
  .note.warn { color: var(--warn); }
  .choices { border: 1px solid var(--line); border-radius: var(--radius-group); background: var(--surface); color: var(--ink); }
  .field { width: 240px; flex: none; }
  .lead { margin-right: auto; margin-left: -8px; }
  .adding { margin-right: auto; align-self: center; color: var(--ink-2); font-size: var(--text-sm); }
</style>
