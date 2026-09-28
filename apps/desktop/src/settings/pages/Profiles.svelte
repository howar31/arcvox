<script lang="ts">
  import { onMount } from "svelte";
  import { deleteProfile, errorMessage, runningApps, setDefaultProfile, upsertProfile, type Profile, type RunningApp, type StepSpec } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import { config } from "../../lib/store.svelte";

  let editing = $state<Profile | null>(null);
  let status = $state("");
  let apps = $state<RunningApp[]>([]);
  let newAppId = $state("");
  let newUrl = $state("");
  let newRaw = $state("");
  let newRefined = $state("");

  onMount(async () => { apps = await runningApps(); });

  const asrProviders = $derived((config.value?.providers ?? []).filter((p) => p.capabilities.transcribe));
  const llmProviders = $derived((config.value?.providers ?? []).filter((p) => p.capabilities.chat));

  function blank(): Profile {
    const base = config.value!.profiles.find((p) => p.is_default)!;
    return { id: "", name: "", is_default: false, matcher: { app_ids: [], url_patterns: [] }, asr_provider: base.asr_provider,
      llm_provider: base.llm_provider, language_hint: "zh-TW", prompt_addition: "", few_shot: [], steps: structuredClone($state.snapshot(base.steps)) };
  }
  function edit(p: Profile) { editing = structuredClone($state.snapshot(p)); status = ""; }
  async function save() {
    if (!editing) return;
    const p = editing;
    if (!p.id) p.id = p.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || `profile-${Date.now()}`;
    if (p.llm_provider === "") p.llm_provider = null;
    if (p.language_hint === "") p.language_hint = null;
    try { await upsertProfile(p); status = t("common.saved"); editing = null; } catch (e) { status = errorMessage(e); }
  }
  async function remove(p: Profile) { try { await deleteProfile(p.id); } catch (e) { status = errorMessage(e); } }
  async function makeDefault(p: Profile) { try { await setDefaultProfile(p.id); } catch (e) { status = errorMessage(e); } }
  function stepLabel(s: StepSpec) { return s.step === "opencc" ? `opencc (${s.config})` : s.step; }
  function moveStep(i: number, d: number) {
    const s = editing!.steps; const j = i + d; if (j < 0 || j >= s.length) return; [s[i], s[j]] = [s[j], s[i]];
  }
  function addStep(kind: string) {
    const s: StepSpec = kind === "opencc" ? { step: "opencc", config: "s2twp" } : kind === "llm" ? { step: "llm" } : { step: "rules" };
    editing!.steps.push(s);
  }
</script>

<h2>{t("nav.profiles")}</h2>
<table>
  <thead><tr><th>{t("profiles.name")}</th><th>{t("profiles.matcher")}</th><th></th></tr></thead>
  <tbody>
    {#each config.value?.profiles ?? [] as p}
      <tr>
        <td>{p.name}{p.is_default ? ` (${t("profiles.default")})` : ""}</td>
        <td>{[...p.matcher.app_ids, ...p.matcher.url_patterns].join(", ")}</td>
        <td>
          <button onclick={() => edit(p)}>{t("common.edit")}</button>
          {#if !p.is_default}<button onclick={() => makeDefault(p)}>{t("profiles.make_default")}</button> <button onclick={() => remove(p)}>{t("common.remove")}</button>{/if}
        </td>
      </tr>
    {/each}
  </tbody>
</table>
<button onclick={() => edit(blank())}>{t("profiles.add")}</button>

{#if editing}
<section class="form">
  <label>{t("profiles.name")} <input bind:value={editing.name} /></label>
  <h3>{t("profiles.matcher")}</h3>
  {#each editing.matcher.app_ids as id, i}
    <div class="row"><code>{id}</code><button onclick={() => editing!.matcher.app_ids.splice(i, 1)}>{t("common.remove")}</button></div>
  {/each}
  <div class="row">
    <select bind:value={newAppId}><option value="">{t("general.pick_app")}</option>{#each apps as a}<option value={a.bundle_id}>{a.name}</option>{/each}</select>
    <input placeholder="com.example.app" bind:value={newAppId} />
    <button onclick={() => { if (newAppId) { editing!.matcher.app_ids.push(newAppId); newAppId = ""; } }}>{t("common.add")}</button>
  </div>
  {#each editing.matcher.url_patterns as u, i}
    <div class="row"><code>{u}</code><button onclick={() => editing!.matcher.url_patterns.splice(i, 1)}>{t("common.remove")}</button></div>
  {/each}
  <div class="row">
    <input placeholder="github.com" bind:value={newUrl} />
    <button onclick={() => { if (newUrl) { editing!.matcher.url_patterns.push(newUrl); newUrl = ""; } }}>{t("profiles.add_url")}</button>
  </div>
  <h3>{t("profiles.providers")}</h3>
  <label>ASR <select bind:value={editing.asr_provider}>{#each asrProviders as p}<option value={p.id}>{p.label}</option>{/each}</select></label>
  <label>LLM <select bind:value={editing.llm_provider}><option value={null}>{t("profiles.none")}</option>{#each llmProviders as p}<option value={p.id}>{p.label}</option>{/each}</select></label>
  <label>{t("profiles.language_hint")} <select bind:value={editing.language_hint}><option value={null}>{t("profiles.none")}</option><option value="zh-TW">zh-TW</option><option value="en">en</option><option value="ja">ja</option></select></label>
  <h3>{t("profiles.prompt_addition")}</h3>
  <textarea rows="6" bind:value={editing.prompt_addition}></textarea>
  <h3>{t("profiles.few_shot")}</h3>
  {#each editing.few_shot as ex, i}
    <div class="row"><span>{ex.raw}</span><span>→</span><span>{ex.refined}</span><button onclick={() => editing!.few_shot.splice(i, 1)}>{t("common.remove")}</button></div>
  {/each}
  <div class="row">
    <input placeholder={t("profiles.raw")} bind:value={newRaw} /><input placeholder={t("profiles.refined")} bind:value={newRefined} />
    <button onclick={() => { if (newRaw && newRefined) { editing!.few_shot.push({ raw: newRaw, refined: newRefined }); newRaw = ""; newRefined = ""; } }}>{t("common.add")}</button>
  </div>
  <h3>{t("profiles.steps")}</h3>
  {#each editing.steps as s, i}
    <div class="row"><code>{stepLabel(s)}</code>
      {#if s.step !== "asr"}<button onclick={() => moveStep(i, -1)}>↑</button><button onclick={() => moveStep(i, 1)}>↓</button><button onclick={() => editing!.steps.splice(i, 1)}>{t("common.remove")}</button>{/if}
    </div>
  {/each}
  <div class="row">
    <button onclick={() => addStep("opencc")}>+ opencc</button><button onclick={() => addStep("llm")}>+ llm</button><button onclick={() => addStep("rules")}>+ rules</button>
  </div>
  <div class="row"><button class="primary" onclick={save}>{t("common.save")}</button><button onclick={() => (editing = null)}>{t("common.cancel")}</button></div>
</section>
{/if}
<p class="status">{status}</p>
