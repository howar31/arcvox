<script lang="ts">
  import { tick } from "svelte";
  import type { InstalledModel, ManifestView, ProviderConfig } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import { placeMenu, type Placement } from "../../../lib/popover";
  import { engineDescription, engineGroups, engineKind, type Installed } from "../../../lib/stages";
  import Icon from "../../../lib/ui/Icon.svelte";
  import Tag from "../../../lib/ui/Tag.svelte";

  /**
   * The engine a stage runs on, as a button that opens the list of engines to choose from, with a line
   * under it about the chosen one. `follow`, when given, adds a first choice that follows the default
   * engine (selected as ""); `none` adds a last choice that selects no engine (selected as null).
   */
  let { engines, selected, onpick, label, manifest, installed, models = [], follow, none, stale, suggested, defaultId, onmanage }: {
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
  } = $props();

  // The list gets a search field from this many engines on.
  const SEARCH_FROM = 9;

  let open = $state(false);
  let query = $state("");
  let root: HTMLDivElement | undefined = $state();
  let button: HTMLButtonElement | undefined = $state();
  let list: HTMLDivElement | undefined = $state();
  // Where the open list sits in the window; null while its natural height is being measured.
  let place = $state<Placement | null>(null);
  let measuring = $state(false);

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
  const options = () => [...(list?.querySelectorAll<HTMLButtonElement>("[role=option]") ?? [])];

  async function show() {
    query = "";
    open = true;
    // The list is measured at its final width before it is placed.
    const anchor = button!.getBoundingClientRect();
    const view = { width: window.innerWidth, height: window.innerHeight };
    place = { ...placeMenu(anchor, 0, view), top: null, bottom: null };
    measuring = true;
    await tick();
    if (list) place = placeMenu(anchor, list.offsetHeight, view);
    measuring = false;
    const field = list?.querySelector("input");
    if (field) field.focus();
    else (options().find((o) => o.getAttribute("aria-selected") === "true") ?? options()[0])?.focus();
    list?.querySelector("[aria-selected=true]")?.scrollIntoView({ block: "nearest" });
  }
  function hide(refocus = true) {
    if (!open) return;
    open = false;
    if (refocus) button?.focus();
  }
  /** The list is fixed in the window: a scroll elsewhere would leave it behind, so it closes. */
  function onscroll(e: Event) {
    if (list && e.target instanceof Node && list.contains(e.target)) return;
    hide(false);
  }
  function pick(id: string | null) {
    hide();
    if (stale || id !== selected) onpick(id);
  }
  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") { hide(); e.stopPropagation(); return; }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const all = options();
    if (all.length === 0) return;
    const at = all.indexOf(document.activeElement as HTMLButtonElement);
    const next = e.key === "ArrowDown" ? (at + 1) % all.length : at <= 0 ? all.length - 1 : at - 1;
    all[next].focus();
  }

  $effect(() => {
    if (!open) return;
    const outside = (e: Event) => { if (root && !root.contains(e.target as Node)) hide(false); };
    const blur = () => hide(false);
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("scroll", onscroll, true);
    window.addEventListener("blur", blur);
    window.addEventListener("resize", blur);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("scroll", onscroll, true);
      window.removeEventListener("blur", blur);
      window.removeEventListener("resize", blur);
    };
  });
</script>

<div class="picker" bind:this={root}>
  <button type="button" class="current" bind:this={button} aria-haspopup="listbox" aria-expanded={open} aria-label={label + " " + name} onclick={() => (open ? hide() : show())}>
    <span class="label">{name}</span>
    {#if stale}<Tag tone="live">{t("home.not_found")}</Tag>
    {:else if chosen && chosenKind?.reason}<Tag tone="warn">{t(chosenKind.reason === "encoder" ? "flow.engine_reason_encoder" : "flow.engine_reason_download")}</Tag>
    {:else if chosen && chosen.id === suggested}<Tag tone="accent" title={t("flow.suggested_tip")}>{t("flow.suggested")}</Tag>{/if}
    <span class="count">{t("flow.engine_count", { count: String(engines.length) })}</span>
    <Icon name="chevron-down" size={14} />
  </button>
  <p class="help" class:warn={stale || chosenKind?.missing}>{help}</p>
  {#if open}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="menu" bind:this={list} {onkeydown}
      style:top={place?.top != null ? `${place.top}px` : null} style:bottom={place?.bottom != null ? `${place.bottom}px` : null}
      style:left={place ? `${place.left}px` : null} style:width={place ? `${place.width}px` : null} style:max-height={place?.maxHeight != null ? `${place.maxHeight}px` : null}
      style:visibility={measuring ? "hidden" : null}
    >
      {#if engines.length >= SEARCH_FROM}
        <input type="search" bind:value={query} placeholder={t("engines.search")} aria-label={t("engines.search")} />
      {/if}
      <div class="options" role="listbox" aria-label={label}>
        {#if follow && !query}
          {@render option("", follow.name, follow.help, selected === "" && !stale)}
        {/if}
        {#each shown as g (g.kind)}
          <div class="head">{groupName(g.kind)}</div>
          {#each g.engines as p (p.id)}
            {@const kind = engineKind(p, installed)}
            {@render option(p.id, p.label, describe(p, kind, false), selected === p.id && !stale, p, kind.reason)}
          {/each}
        {:else}
          {#if query}<p class="none">{t("engines.no_match")}</p>{/if}
        {/each}
        {#if none && !query}
          <div class="rule"></div>
          {@render option(null, none.name, none.help, selected === null && !stale)}
        {/if}
      </div>
      <div class="foot">
        <span>{t("flow.engine_count", { count: String(engines.length) })}</span>
        {#if onmanage}<button type="button" class="btn quiet" onclick={() => { hide(); onmanage?.(); }}>{t("flow.manage")}</button>{/if}
      </div>
    </div>
  {/if}
</div>

{#snippet option(id: string | null, text: string, small: string, on: boolean, p?: ProviderConfig, reason?: "download" | "encoder")}
  <button type="button" class="option" class:on class:dim={reason} role="option" aria-selected={on} onclick={() => pick(id)}>
    <span class="tick">{#if on}<Icon name="check" size={14} />{/if}</span>
    <span class="text">
      <span class="line">
        <span class="label">{text}</span>
        {#if p && p.id === suggested}<Tag tone="accent" title={t("flow.suggested_tip")}>{t("flow.suggested")}</Tag>{/if}
        {#if p && p.id === defaultId}<Tag tone="ok">{t("engines.default_tag")}</Tag>{/if}
        {#if p?.model_path}<Tag title={t("engines.tip.imported")}>{t("engines.tag.imported")}</Tag>{/if}
        {#if reason}<Tag tone="warn">{t(reason === "encoder" ? "flow.engine_reason_encoder" : "flow.engine_reason_download")}</Tag>{/if}
      </span>
      <span class="where">{small}</span>
    </span>
  </button>
{/snippet}

<style>
  .picker { position: relative; }
  .current { display: flex; align-items: center; gap: 10px; width: 100%; height: 44px; padding: 0 14px; border-radius: 12px; border: 1px solid var(--line-strong); background: var(--surface); color: var(--ink); font-size: var(--text); font-weight: 600; cursor: default; }
  .current:hover, .current[aria-expanded="true"] { background: var(--sunken); }
  .label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count { margin-left: auto; flex: none; color: var(--ink-3); font-size: var(--text-sm); font-weight: 400; white-space: nowrap; }
  .current :global(svg) { flex: none; color: var(--ink-3); }
  .help { margin: 8px 4px 0; color: var(--ink-2); font-size: var(--text-sm); overflow-wrap: anywhere; }
  .help.warn { color: var(--warn); }
  .menu { position: fixed; z-index: 20; display: flex; flex-direction: column; padding: 6px; border-radius: 14px; border: 1px solid var(--line); background: var(--surface); box-shadow: var(--shadow); }
  .menu input { flex: none; width: 100%; margin-bottom: 4px; }
  .options { overflow-y: auto; overscroll-behavior: contain; min-height: 0; }
  .head { padding: 10px 12px 4px; color: var(--ink-3); font-size: var(--text-xs); font-weight: 600; }
  .rule { height: 1px; margin: 6px 8px; background: var(--line); }
  .option { display: flex; align-items: flex-start; gap: 8px; width: 100%; padding: 7px 12px 7px 8px; border: 0; border-radius: 9px; background: none; color: var(--ink); text-align: left; font-size: var(--text-control); cursor: default; }
  .option:hover, .option:focus-visible { background: var(--sunken); }
  .option.on { background: var(--accent-soft); }
  .option.dim .label { color: var(--ink-3); }
  .tick { flex: none; width: 16px; height: 22px; display: grid; place-items: center; color: var(--accent); }
  .text { min-width: 0; flex: 1; }
  .line { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .where { display: block; color: var(--ink-2); font-size: var(--text-xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .none { margin: 0; padding: 12px; color: var(--ink-2); font-size: var(--text-sm); }
  .foot { flex: none; display: flex; align-items: center; justify-content: space-between; gap: 8px; margin: 6px -6px -6px; padding: 4px 6px 4px 14px; border-top: 1px solid var(--line); background: var(--sunken); border-radius: 0 0 14px 14px; color: var(--ink-2); font-size: var(--text-sm); }
</style>
