<script lang="ts">
  import type { Profile } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import { wantsSearch } from "../../../lib/dropdown";
  import { tip } from "../../../lib/tip";
  import Icon from "../../../lib/ui/Icon.svelte";
  import Bang from "../../../lib/ui/Bang.svelte";
  import Dropdown from "../../../lib/ui/Dropdown.svelte";
  import MenuOption from "../../../lib/ui/MenuOption.svelte";
  import Tag from "../../../lib/ui/Tag.svelte";

  /** The profile being edited, as a button that opens the list of all profiles. */
  let { profiles, current, name, applies, onpick, onmanage, onadd, faulty = new Set(), moved = new Set() }: {
    profiles: Profile[];
    current: string;
    /** The name shown for the current profile, which may be an unsaved edit. */
    name: string;
    applies: (p: Profile) => string;
    onpick: (id: string) => void;
    /** Opens the list where several profiles are managed at once. */
    onmanage: () => void;
    /** Opens the choice of a blank profile or a preset. */
    onadd: () => void;
    /** Profiles running on the default engine because an engine they name cannot run. */
    faulty?: Set<string>;
    /** The stored profiles with a writing style of their own whose template's style has moved on. */
    moved?: Set<string>;
  } = $props();

  let query = $state("");

  const isDefault = $derived(profiles.find((p) => p.id === current)?.is_default ?? false);
  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return q ? profiles.filter((p) => (p.name + " " + applies(p)).toLowerCase().includes(q)) : profiles;
  });

</script>

<div class="picker">
  <Dropdown label={t("flow.profiles") + " " + name} large minWidth={420} bind:query search={wantsSearch(profiles.length, true)} placeholder={t("flow.search")}>
    {#snippet button()}
      <span class="label" use:tip={name}>{name}</span>
      {#if isDefault}<Tag tone="accent">{t("profiles.default")}</Tag>{/if}
      <!-- The closed list says, beside its count, when any profile in it runs on the default engine or keeps a style its template has moved past. -->
      <span class="count">{#if faulty.size > 0 || moved.size > 0}<Bang tone="warn" title={[faulty.size > 0 ? t("flow.profiles_fallback", { count: String(faulty.size) }) : "", moved.size > 0 ? t("flow.profiles_style_moved", { count: String(moved.size) }) : ""].filter(Boolean).join(" ")} />{/if}{t("flow.count", { count: String(profiles.length) })}</span>
    {/snippet}
    {#snippet list(close)}
      {#each shown as p (p.id)}
        <MenuOption selected={p.id === current} onclick={() => { close(); if (p.id !== current) onpick(p.id); }}>
          <span class="line"><span class="label">{p.id === current ? name : p.name}</span>{#if p.is_default}<Tag tone="accent">{t("profiles.default")}</Tag>{/if}{#if faulty.has(p.id)}<Bang tone="warn" title={t("flow.profile_fallback")} />{:else if moved.has(p.id)}<Bang tone="warn" title={t("flow.style_moved")} />{/if}</span>
          <span class="where">{applies(p)}</span>
        </MenuOption>
      {:else}
        <p class="none">{t("flow.no_match")}</p>
      {/each}
    {/snippet}
    {#snippet foot(close)}
      <span>{t("flow.count", { count: String(profiles.length) })}</span>
      <span class="acts">
        <button type="button" class="btn quiet" onclick={() => { close(); onadd(); }}><Icon name="plus" size={14} />{t("flow.add")}</button>
        <button type="button" class="btn quiet" onclick={() => { close(); onmanage(); }}>{t("flow.manage_profiles")}</button>
      </span>
    {/snippet}
  </Dropdown>
</div>

<style>
  .picker { flex: 1 1 auto; min-width: 0; }
  .label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count { display: inline-flex; align-items: center; gap: 7px; margin-left: auto; flex: none; color: var(--ink-3); font-size: var(--text-sm); font-weight: 400; white-space: nowrap; }
  /* A long name wraps in the list; the tag and the mark stay beside its first line. */
  .line { display: flex; align-items: flex-start; gap: 8px; }
  .line .label { white-space: normal; overflow-wrap: anywhere; padding-top: 1px; }
  .line :global(.bang) { margin-top: 4px; }
  .where { display: block; color: var(--ink-2); font-size: var(--text-xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .none { margin: 0; padding: 12px; color: var(--ink-2); font-size: var(--text-sm); }
  .acts { display: flex; gap: 2px; }
</style>
