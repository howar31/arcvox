<script lang="ts">
  import { onMount, tick } from "svelte";
  import { errorMessage, runningApps, type RunningApp } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import { isImeKey } from "../../lib/ime";
  import Row from "../../lib/ui/Row.svelte";
  import AppIcon from "../../lib/ui/AppIcon.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  /**
   * One button that opens the list of running applications; picking one adds it at once. A bundle
   * id typed into the search that matches nothing can be added as typed. `exclude` holds the bundle
   * ids already in the page's list; they are left out.
   */
  let { onadd, exclude = [] }: { onadd: (id: string) => void; exclude?: string[] } = $props();
  let apps = $state<RunningApp[]>([]);
  onMount(async () => {
    try { apps = await runningApps(); } catch (e) { toast.error(errorMessage(e)); }
  });

  // The list opens upwards when less than this is free below the button.
  const MENU_ROOM = 320;

  let open = $state(false);
  let upwards = $state(false);
  let query = $state("");
  let root: HTMLDivElement | undefined = $state();
  let button: HTMLButtonElement | undefined = $state();
  let list: HTMLDivElement | undefined = $state();

  const offered = $derived(apps.filter((a) => !exclude.includes(a.bundle_id)));
  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return q ? offered.filter((a) => (a.name + " " + a.bundle_id).toLowerCase().includes(q)) : offered;
  });
  // A typed bundle id (a dotted name) that is no listed application can be added as typed.
  const typedId = $derived.by(() => {
    const q = query.trim();
    return q.includes(".") && !/\s/.test(q) && !shown.some((a) => a.bundle_id === q) && !exclude.includes(q) ? q : "";
  });

  const options = () => [...(list?.querySelectorAll<HTMLButtonElement>("[role=option]") ?? [])];

  async function show() {
    query = "";
    const box = button?.getBoundingClientRect();
    upwards = !!box && window.innerHeight - box.bottom < MENU_ROOM && box.top > window.innerHeight - box.bottom;
    open = true;
    await tick();
    list?.querySelector("input")?.focus();
  }
  function hide(refocus = true) {
    if (!open) return;
    open = false;
    if (refocus) button?.focus();
  }
  function pick(id: string) {
    onadd(id);
    hide();
  }
  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") { hide(); e.stopPropagation(); return; }
    if (isImeKey(e)) return;
    const all = options();
    if (e.key === "Enter" && document.activeElement instanceof HTMLInputElement) {
      e.preventDefault();
      if (all.length > 0) all[0].click();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
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

<Row>
  <div class="apps" bind:this={root}>
    <button type="button" class="btn" bind:this={button} aria-haspopup="listbox" aria-expanded={open} onclick={() => (open ? hide() : show())}>
      <Icon name="plus" size={14} />{t("general.add_app")}
    </button>
    {#if open}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="menu" class:upwards bind:this={list} {onkeydown}>
        <input type="search" bind:value={query} placeholder={t("general.search_app")} aria-label={t("general.search_app")} />
        <div class="options" role="listbox" aria-label={t("general.add_app")}>
          {#each shown as a (a.bundle_id)}
            <button type="button" class="option" role="option" aria-selected="false" onclick={() => pick(a.bundle_id)}>
              <AppIcon bundleId={a.bundle_id} size={24} />
              <span class="text">
                <span class="name">{a.name}</span>
                <span class="id">{a.bundle_id}</span>
              </span>
            </button>
          {/each}
          {#if typedId}
            <button type="button" class="option typed" role="option" aria-selected="false" onclick={() => pick(typedId)}>
              <Icon name="plus" size={14} />
              <span class="text"><span class="name">{t("general.add_id", { id: typedId })}</span></span>
            </button>
          {:else if shown.length === 0}
            <p class="none">{t(offered.length === 0 ? "general.no_apps" : "general.no_app_match")}</p>
          {/if}
        </div>
      </div>
    {/if}
  </div>
</Row>

<style>
  .apps { position: relative; }
  .menu { position: absolute; top: calc(100% + 6px); left: 0; z-index: 20; display: flex; flex-direction: column; width: 340px; max-width: calc(100vw - 2 * var(--page-x) - 248px); max-height: 300px; padding: 6px; border-radius: 14px; border: 1px solid var(--line); background: var(--surface); box-shadow: var(--shadow); }
  .menu.upwards { top: auto; bottom: calc(100% + 6px); }
  .menu input { flex: none; width: 100%; margin-bottom: 4px; }
  .options { overflow-y: auto; min-height: 0; }
  .option { display: flex; align-items: center; gap: 10px; width: 100%; padding: 6px 10px; border: 0; border-radius: 9px; background: none; color: var(--ink); text-align: left; font-size: var(--text-control); cursor: default; }
  .option .text { min-width: 0; flex: 1; }
  .option:hover, .option:focus-visible { background: var(--sunken); }
  .option.typed :global(svg) { flex: none; width: 24px; color: var(--ink-2); }
  .name { display: block; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .id { display: block; color: var(--ink-2); font-size: var(--text-xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .none { margin: 0; padding: 12px; color: var(--ink-2); font-size: var(--text-sm); }
</style>
