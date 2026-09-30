<script lang="ts">
  import { onMount } from "svelte";
  import { errorMessage, profilesAddPreset, profilesPresets, type PresetView, type Profile } from "../../lib/api";
  import { appName, ensureAppNames } from "../../lib/apps.svelte";
  import { t } from "../../lib/i18n";
  import { appsSummary, NEW_PROFILE } from "../../lib/profileEdit";
  import { go } from "../../lib/route";
  import { config } from "../../lib/store.svelte";
  import Empty from "../../lib/ui/Empty.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Modal from "../../lib/ui/Modal.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Ring from "../../lib/ui/Ring.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  let presets = $state<PresetView[]>([]);
  let adding = $state(false);
  let busy = $state(false);

  onMount(() => {
    profilesPresets().then((p) => { presets = p; }).catch((e) => toast.error(t("profiles.presets_failed", { detail: errorMessage(e) })));
  });

  // The default profile first, the others in their stored order.
  const profiles = $derived([...(config.value?.profiles ?? [])].sort((a, b) => Number(b.is_default) - Number(a.is_default)));
  const providers = $derived(config.value?.providers ?? []);

  $effect(() => { ensureAppNames(profiles.flatMap((p) => p.matcher.app_ids)); });

  const label = (id: string | null) => (id === null ? t("home.no_rewrite") : providers.find((p) => p.id === id)?.label ?? null);

  function appsLine(p: Profile): string {
    if (p.is_default) return t("profiles.everywhere");
    const names = [...p.matcher.app_ids.map(appName), ...p.matcher.url_patterns];
    if (names.length === 0) return t("profiles.no_apps");
    const { shown, more } = appsSummary(names);
    const list = shown.join(t("home.list_sep"));
    return more > 0 ? t("home.apps_more", { names: list, count: String(more) }) : list;
  }

  function addBlank() {
    adding = false;
    go("profiles", NEW_PROFILE);
  }

  async function addPreset(id: string) {
    busy = true;
    const before = new Set((config.value?.profiles ?? []).map((p) => p.id));
    try {
      const next = await profilesAddPreset(id, t(`profiles.preset.${id}`));
      // Store the result first so the detail page finds the new profile without waiting for the change event.
      config.value = next;
      // The backend suffixes the id when it is taken, so the new profile is the one that was not there before.
      const created = next.profiles.find((p) => !before.has(p.id));
      adding = false;
      toast.ok(t("common.saved"));
      if (created) go("profiles", created.id);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      busy = false;
    }
  }
</script>

{#snippet engineName(id: string | null)}
  {@const name = label(id)}
  <span class="eng" class:missing={name === null}>
    {#if name === null}<span class="line unknown">{id}</span><Tag tone="warn">{t("home.not_found")}</Tag>
    {:else}<span class="line">{name}</span>{/if}
  </span>
{/snippet}

<PageHead title={t("nav.profiles")} description={t("profiles.description")}>
  {#snippet actions()}
    <button type="button" class="btn primary" disabled={!config.value} onclick={() => (adding = true)}><Icon name="plus" size={14} />{t("profiles.add")}</button>
  {/snippet}
</PageHead>

{#if config.value}
  <Group>
    {#each profiles as p (p.id)}
      <Row onclick={() => go("profiles", p.id)}>
        {#snippet leading()}<Ring tone={p.is_default ? "accent" : "off"} />{/snippet}
        <span class="who">
          <span class="title">
            <span class="name line">{p.name}</span>
            {#if p.is_default}<Tag tone="accent">{t("profiles.default")}</Tag>{/if}
          </span>
          <span class="sub line">{appsLine(p)}</span>
        </span>
        <span class="pair">{@render engineName(p.asr_provider)}<span class="arrow" aria-hidden="true">→</span>{@render engineName(p.llm_provider)}</span>
      </Row>
    {/each}
  </Group>
  {#if profiles.length < 2}
    <Group>
      <Empty text={t("profiles.only_default")}>
        {#snippet action()}<button type="button" class="btn" onclick={() => (adding = true)}><Icon name="plus" size={14} />{t("profiles.add")}</button>{/snippet}
      </Empty>
    </Group>
  {/if}
{/if}

<Modal bind:open={adding} title={t("profiles.add")}>
  <div class="choices" class:busy inert={busy} aria-busy={busy}>
    <Row label={t("profiles.blank")} help={t("profiles.blank.desc")} onclick={addBlank} />
    {#each presets as p (p.id)}
      <Row label={t(`profiles.preset.${p.id}`)} help={t(`profiles.preset.${p.id}.desc`)} onclick={() => addPreset(p.id)} />
    {/each}
  </div>
  {#snippet actions()}
    {#if busy}<span class="adding" role="status">{t("profiles.adding")}</span>{/if}
    <button type="button" class="btn" disabled={busy} onclick={() => (adding = false)}>{t("common.cancel")}</button>
  {/snippet}
</Modal>

<style>
  /* The name keeps its room up to a limit; the engine pair gives way first. */
  .who { display: flex; flex-direction: column; flex: 1 0 auto; min-width: 0; max-width: 50%; }
  /* The applications line does not widen the name column. */
  .who > .sub { contain: inline-size; }
  .title { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .line { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  /* The two engine names share the space; each truncates on its own, before the profile name does. */
  .pair { display: flex; align-items: center; gap: 5px; flex: 0 1 auto; min-width: 0; max-width: 52%; overflow: hidden; color: var(--ink-2); }
  .eng { display: flex; align-items: center; gap: 5px; flex: 0 1 auto; min-width: 0; max-width: 50%; }
  /* A missing engine keeps its tag and a short id; the other name gives way. */
  .eng.missing { flex: none; max-width: none; }
  .arrow { flex: none; color: var(--ink-3); }
  /* An id that is not found keeps a few characters next to its tag. */
  .unknown { color: var(--warn); max-width: 5em; }
  .choices.busy { opacity: .55; }
  .adding { margin-right: auto; align-self: center; color: var(--ink-2); font-size: 12px; }
  .choices { margin-top: 8px; border: 1px solid var(--line); border-radius: 10px; background: var(--surface); color: var(--ink); }
</style>
