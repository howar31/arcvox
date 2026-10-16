<script lang="ts">
  import { tick } from "svelte";
  import type { Profile } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import Icon from "../../../lib/ui/Icon.svelte";
  import Bang from "../../../lib/ui/Bang.svelte";
  import Tag from "../../../lib/ui/Tag.svelte";

  /** The profile being edited, as a button that opens the list of all profiles. */
  let { profiles, current, name, applies, onpick, onmanage, onadd, faulty = new Set() }: {
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
  } = $props();

  // The list gets a search field from this many profiles on.
  const SEARCH_FROM = 9;

  let open = $state(false);
  let query = $state("");
  let root: HTMLDivElement | undefined = $state();
  let button: HTMLButtonElement | undefined = $state();
  let list: HTMLDivElement | undefined = $state();

  const isDefault = $derived(profiles.find((p) => p.id === current)?.is_default ?? false);
  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return q ? profiles.filter((p) => (p.name + " " + applies(p)).toLowerCase().includes(q)) : profiles;
  });

  const options = () => [...(list?.querySelectorAll<HTMLButtonElement>("[role=option]") ?? [])];

  async function show() {
    query = "";
    open = true;
    await tick();
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
  function pick(id: string) {
    hide();
    if (id !== current) onpick(id);
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
    window.addEventListener("blur", blur);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("blur", blur);
    };
  });
</script>

<div class="picker" bind:this={root}>
  <button type="button" class="current" bind:this={button} aria-haspopup="listbox" aria-expanded={open} aria-label={t("flow.profiles") + " " + name} onclick={() => (open ? hide() : show())}>
    <span class="label">{name}</span>
    {#if isDefault}<Tag tone="accent">{t("profiles.default")}</Tag>{/if}
    <!-- The closed list says, beside its count, when any profile in it runs on the default engine. -->
    <span class="count">{#if faulty.size > 0}<Bang title={t("flow.profiles_fallback", { count: String(faulty.size) })} />{/if}{t("flow.count", { count: String(profiles.length) })}</span>
    <Icon name="chevron-down" size={14} />
  </button>
  {#if open}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="menu" bind:this={list} {onkeydown}>
      {#if profiles.length >= SEARCH_FROM}
        <input type="search" bind:value={query} placeholder={t("flow.search")} aria-label={t("flow.search")} />
      {/if}
      <div class="options" role="listbox" aria-label={t("flow.profiles")}>
        {#each shown as p (p.id)}
          <button type="button" class="option" class:on={p.id === current} role="option" aria-selected={p.id === current} onclick={() => pick(p.id)}>
            <span class="tick">{#if p.id === current}<Icon name="check" size={14} />{/if}</span>
            <span class="text">
              <span class="line"><span class="label">{p.id === current ? name : p.name}</span>{#if p.is_default}<Tag tone="accent">{t("profiles.default")}</Tag>{/if}{#if faulty.has(p.id)}<Bang title={t("flow.profile_fallback")} />{/if}</span>
              <span class="where">{applies(p)}</span>
            </span>
          </button>
        {:else}
          <p class="none">{t("flow.no_match")}</p>
        {/each}
      </div>
      <div class="foot">
        <span>{t("flow.count", { count: String(profiles.length) })}</span>
        <span class="acts">
          <button type="button" class="btn quiet" onclick={() => { hide(); onadd(); }}><Icon name="plus" size={14} />{t("flow.add")}</button>
          <button type="button" class="btn quiet" onclick={() => { hide(); onmanage(); }}>{t("flow.manage_profiles")}</button>
        </span>
      </div>
    </div>
  {/if}
</div>

<style>
  .picker { position: relative; flex: 0 1 360px; min-width: 0; }
  .current { display: flex; align-items: center; gap: 10px; width: 100%; height: 44px; padding: 0 14px; border-radius: 12px; border: 1px solid var(--line-strong); background: var(--surface); color: var(--ink); font-size: var(--text); font-weight: 600; cursor: default; }
  .current:hover, .current[aria-expanded="true"] { background: var(--sunken); }
  .label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count { display: inline-flex; align-items: center; gap: 7px; margin-left: auto; flex: none; color: var(--ink-3); font-size: var(--text-sm); font-weight: 400; white-space: nowrap; }
  .current :global(svg) { flex: none; color: var(--ink-3); }
  .menu { position: absolute; top: 50px; left: 0; z-index: 20; display: flex; flex-direction: column; width: max(100%, 420px); max-width: calc(100vw - 2 * var(--page-x) - 248px); max-height: 340px; padding: 6px; border-radius: 14px; border: 1px solid var(--line); background: var(--surface); box-shadow: var(--shadow); }
  .menu input { flex: none; width: 100%; margin-bottom: 4px; }
  .options { overflow-y: auto; min-height: 0; }
  .option { display: flex; align-items: flex-start; gap: 8px; width: 100%; padding: 8px 12px 8px 8px; border: 0; border-radius: 9px; background: none; color: var(--ink); text-align: left; font-size: var(--text-control); cursor: default; }
  .option:hover, .option:focus-visible { background: var(--sunken); }
  .option.on { background: var(--accent-soft); }
  .tick { flex: none; width: 16px; height: 22px; display: grid; place-items: center; color: var(--accent); }
  .text { min-width: 0; flex: 1; }
  .line { display: flex; align-items: center; gap: 8px; }
  .where { display: block; color: var(--ink-2); font-size: var(--text-xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .none { margin: 0; padding: 12px; color: var(--ink-2); font-size: var(--text-sm); }
  .foot .acts { display: flex; gap: 2px; }
  .foot { flex: none; display: flex; align-items: center; justify-content: space-between; gap: 8px; margin: 6px -6px -6px; padding: 4px 6px 4px 14px; border-top: 1px solid var(--line); background: var(--sunken); border-radius: 0 0 14px 14px; color: var(--ink-2); font-size: var(--text-sm); }
</style>
