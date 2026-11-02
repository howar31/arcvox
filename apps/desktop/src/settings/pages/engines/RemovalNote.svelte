<script lang="ts">
  /** What deleting network engines does, said before it is done: which engines, and where the profiles that name them go. */
  import type { PresetView, ProviderConfig } from "../../../lib/api";
  import type { Role } from "../../../lib/engines";
  import { namingProfiles, templateOffers, type MoveTo } from "../../../lib/engineRemoval";
  import { t } from "../../../lib/i18n";
  import { config } from "../../../lib/store.svelte";
  import Choices from "../../../lib/ui/Choices.svelte";

  let { engines, role, presets, moveTo = $bindable("default") }: {
    engines: ProviderConfig[];
    role: Role;
    presets: PresetView[];
    moveTo?: MoveTo;
  } = $props();

  const ids = $derived(engines.map((e) => e.id));
  const naming = $derived(namingProfiles(config.value?.profiles ?? [], ids));
  const offers = $derived(templateOffers(naming, ids, presets));
  const defaultName = $derived.by(() => {
    const id = config.value?.default_engines[role];
    return config.value?.providers.find((p) => p.id === id)?.label ?? id ?? "";
  });
  const names = $derived(engines.map((e) => e.label).join(t("home.list_sep")));
</script>

<p class="ask">{engines.length > 1 ? t("engines.delete_n_body", { names }) : t("engines.delete_body", { name: names })}</p>
{#if naming.length}
  <p class="ask">{t(engines.length > 1 ? "engines.delete_n_moves" : "engines.delete_moves", { names: naming.map((p) => p.name).join(t("home.list_sep")) })}</p>
  {#if offers}
    <Choices
      label={t("engines.delete_title")}
      bind:value={moveTo}
      options={[
        { value: "default" as const, label: t("engines.delete_to_default"), help: t("engines.delete_to_default_help", { kind: t(`engines.group.${role}`), name: defaultName }) },
        { value: "template" as const, label: t("engines.delete_to_template"), help: t("engines.delete_to_template_help") },
      ]}
    />
  {:else}
    <p class="ask">{t("engines.delete_to_default_help", { kind: t(`engines.group.${role}`), name: defaultName })}</p>
  {/if}
{/if}

<style>
  .ask { margin: 0 0 10px; color: var(--ink-2); overflow-wrap: anywhere; }
</style>
