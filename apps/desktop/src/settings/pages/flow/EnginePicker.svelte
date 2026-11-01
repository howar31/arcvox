<script lang="ts">
  import type { InstalledModel, ManifestView, ProviderConfig } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import { wantsSearch } from "../../../lib/dropdown";
  import { engineDescription, engineGroups, engineKind, type Installed } from "../../../lib/stages";
  import Dropdown from "../../../lib/ui/Dropdown.svelte";
  import MenuOption from "../../../lib/ui/MenuOption.svelte";
  import Tag from "../../../lib/ui/Tag.svelte";

  /**
   * The engine a stage runs on, as a button that opens the list of engines to choose from, with a line
   * under it about the chosen one. `follow`, when given, adds a first choice that follows the default
   * engine (selected as ""); `none` adds a last choice that selects no engine (selected as null).
   */
  let { engines, selected, onpick, label, manifest, installed, models = [], follow, none, stale, suggested, defaultId, onmanage, compact = false }: {
    engines: ProviderConfig[];
    selected: string | null;
    onpick: (id: string | null) => void;
    /** Accessible name of the control. */
    label: string;
    manifest: ManifestView | null;
    installed: Installed;
    /** The installed model files, for what a file of the user's own says about itself. */
    models?: InstalledModel[];
    follow?: { name: string; help: string };
    none?: { name: string; help: string };
    /** Shown as the choice when the stored engine is not in `engines`. */
    stale?: { name: string; help: string };
    /** The engine the profile's template was tuned with; it gets the suggested tag. */
    suggested?: string | null;
    /** The default engine of this role; it gets the default tag in the list. */
    defaultId?: string | null;
    /** Opens the Engines page at this role. */
    onmanage?: () => void;
    /** The smaller button, for a row of a group. */
    compact?: boolean;
  } = $props();

  let query = $state("");

  const chosen = $derived(stale ? null : selected === "" || selected === null ? null : engines.find((p) => p.id === selected) ?? null);
  const chosenKind = $derived(chosen ? engineKind(chosen, installed) : null);
  /** What the button says. */
  const name = $derived(stale ? stale.name : selected === "" ? follow?.name ?? "" : selected === null ? none?.name ?? "" : chosen?.label ?? selected);
  /** The line under the button: the chosen engine's kind and what it is, or the choice's own help. */
  const help = $derived(stale ? stale.help : selected === "" ? follow?.help ?? "" : selected === null ? none?.help ?? "" : chosen ? describe(chosen, chosenKind!, true) : "");
  const groups = $derived(engineGroups(engines, installed));
  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return groups;
    return groups.map((g) => ({ ...g, engines: g.engines.filter((p) => (p.label + " " + engineDescription(p, manifest, models)).toLowerCase().includes(q)) })).filter((g) => g.engines.length > 0);
  });

  /** The engine in a sentence; with its kind and state first for the line under the button. */
  function describe(p: ProviderConfig, kind: { meta: string }, withKind: boolean): string {
    const what = engineDescription(p, manifest, models);
    return withKind && p.kind !== "http" ? kind.meta + " · " + what : what;
  }
  const groupName = (kind: ProviderConfig["kind"]) => t(kind === "embedded" ? "engines.local_models" : kind === "native" ? "engines.builtin" : "engines.external");
</script>

<div class="picker" class:compact>
  <Dropdown {label} large={!compact} bind:query search={wantsSearch(engines.length, true)} placeholder={t("engines.search")}>
    {#snippet button()}
      <span class="label">{name}</span>
      {#if stale}<Tag tone="live">{t("home.not_found")}</Tag>
      {:else}
        {#if chosen && chosen.id === suggested}<Tag tone="accent" title={t("flow.suggested_tip")}>{t("flow.suggested")}</Tag>{/if}
        {#if chosen && chosenKind?.reason}<Tag tone="warn">{t(chosenKind.reason === "encoder" ? "flow.engine_reason_encoder" : "flow.engine_reason_download")}</Tag>{/if}
      {/if}
      <span class="count">{t("flow.engine_count", { count: String(engines.length) })}</span>
    {/snippet}
    {#snippet list(close)}
      {#if follow && !query}
        {@render option(close, "", follow.name, follow.help, selected === "" && !stale)}
      {/if}
      {#each shown as g (g.kind)}
        <div class="head">{groupName(g.kind)}</div>
        {#each g.engines as p (p.id)}
          {@const kind = engineKind(p, installed)}
          {@render option(close, p.id, p.label, describe(p, kind, false), selected === p.id && !stale, p, kind.reason)}
        {/each}
      {:else}
        {#if query}<p class="none">{t("engines.no_match")}</p>{/if}
      {/each}
      {#if none && !query}
        <div class="rule"></div>
        {@render option(close, null, none.name, none.help, selected === null && !stale)}
      {/if}
    {/snippet}
    {#snippet foot(close)}
      <span>{t("flow.engine_count", { count: String(engines.length) })}</span>
      {#if onmanage}<button type="button" class="btn quiet" onclick={() => { close(); onmanage?.(); }}>{t("flow.manage")}</button>{/if}
    {/snippet}
  </Dropdown>
  <p class="help" class:warn={stale || chosenKind?.missing}>{help}</p>
</div>

{#snippet option(close: () => void, id: string | null, text: string, small: string, on: boolean, p?: ProviderConfig, reason?: "download" | "encoder")}
  <MenuOption selected={on} onclick={() => { close(); if (stale || id !== selected) onpick(id); }}>
    <span class="line" class:dim={reason}>
      <span class="label">{text}</span>
      {#if p && p.id === suggested}<Tag tone="accent" title={t("flow.suggested_tip")}>{t("flow.suggested")}</Tag>{/if}
      {#if p && p.id === defaultId}<Tag tone="ok">{t("engines.default_tag")}</Tag>{/if}
      {#if p?.model_path}<Tag title={t("engines.tip.imported")}>{t("engines.tag.imported")}</Tag>{/if}
      {#if reason}<Tag tone="warn">{t(reason === "encoder" ? "flow.engine_reason_encoder" : "flow.engine_reason_download")}</Tag>{/if}
    </span>
    <span class="where">{small}</span>
  </MenuOption>
{/snippet}

<style>
  .picker { min-width: 0; }
  .label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count { margin-left: auto; flex: none; color: var(--ink-3); font-size: var(--text-sm); font-weight: 400; white-space: nowrap; }
  .help { margin: 8px 4px 0; color: var(--ink-2); font-size: var(--text-sm); overflow-wrap: anywhere; }
  .help.warn { color: var(--warn); }
  .compact .help { margin-top: 6px; font-size: var(--text-xs); }
  .head { padding: 10px 12px 4px; color: var(--ink-3); font-size: var(--text-xs); font-weight: 600; }
  .rule { height: 1px; margin: 6px 8px; background: var(--line); }
  .line { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .line.dim .label { color: var(--ink-3); }
  .where { display: block; color: var(--ink-2); font-size: var(--text-xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .none { margin: 0; padding: 12px; color: var(--ink-2); font-size: var(--text-sm); }
</style>
