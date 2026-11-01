<script lang="ts">
  import { filterGroups, wantsSearch, type Choice, type ChoiceGroup } from "../dropdown";
  import { t } from "../i18n";
  import Dropdown from "./Dropdown.svelte";
  import MenuOption from "./MenuOption.svelte";

  /** A choice of plain items, in the app's one dropdown. The caller holds the value and takes `onchange`. */
  let { value, options, groups, onchange, label, variable = false, disabled = false, width = "240px" }: {
    value: string;
    options?: Choice[];
    groups?: ChoiceGroup[];
    onchange: (value: string) => void;
    /** Accessible name of the control. */
    label: string;
    /** The list's length depends on the machine or the user. */
    variable?: boolean;
    disabled?: boolean;
    width?: string;
  } = $props();

  let query = $state("");
  const all = $derived(groups ?? [{ choices: options ?? [] }]);
  const count = $derived(all.reduce((n, g) => n + g.choices.length, 0));
  const shown = $derived(filterGroups(all, query));
  const current = $derived(all.flatMap((g) => g.choices).find((c) => c.value === value));
</script>

<div class="select" style:width={width}>
  <Dropdown {label} {disabled} search={wantsSearch(count, variable)} bind:query>
    {#snippet button()}<span class="label">{current?.label ?? value}</span>{/snippet}
    {#snippet list(close)}
      {#each shown as g, i (g.label ?? i)}
        {#if g.label}<div class="head">{g.label}</div>{/if}
        {#each g.choices as c (c.value)}
          <MenuOption selected={c.value === value} disabled={c.disabled} onclick={() => { close(); if (c.value !== value) onchange(c.value); }}>
            {c.label}{#if c.help}<small>{c.help}</small>{/if}
          </MenuOption>
        {/each}
      {:else}
        <p class="none">{t("common.no_match")}</p>
      {/each}
    {/snippet}
  </Dropdown>
</div>

<style>
  .select { flex: none; min-width: 0; max-width: 100%; }
  .label { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .head { padding: 10px 12px 4px; color: var(--ink-3); font-size: var(--text-xs); font-weight: 600; }
  small { display: block; color: var(--ink-2); font-size: var(--text-xs); }
  .none { margin: 0; padding: 12px; color: var(--ink-2); font-size: var(--text-sm); }
</style>
