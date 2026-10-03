<script lang="ts">
  import { onMount, tick } from "svelte";
  import { errorMessage, runningApps, type RunningApp } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import { isImeKey } from "../../lib/ime";
  import Row from "../../lib/ui/Row.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  /**
   * `value` is the bundle id chosen or typed but not added yet; the page binds it so that text counts
   * as an unsaved change and its save can add it first.
   */
  let { onadd, value = $bindable("") }: { onadd: (id: string) => void; value?: string } = $props();
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

  const chosen = $derived(apps.find((a) => a.bundle_id === value.trim()));
  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return q ? apps.filter((a) => (a.name + " " + a.bundle_id).toLowerCase().includes(q)) : apps;
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
    value = id;
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

  function add() {
    const id = value.trim();
    if (!id) return;
    onadd(id);
    value = "";
  }
</script>

<Row>
  <div class="apps" bind:this={root}>
    <button type="button" class="current" class:empty={!chosen} bind:this={button} aria-haspopup="listbox" aria-expanded={open} aria-label={t("general.pick_app")} onclick={() => (open ? hide() : show())}>
      <span class="name">{chosen ? chosen.name : t("general.pick_app")}</span>
      <Icon name="chevron-down" size={14} />
    </button>
    {#if open}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="menu" class:upwards bind:this={list} {onkeydown}>
        <input type="search" bind:value={query} placeholder={t("general.search_app")} aria-label={t("general.search_app")} />
        <div class="options" role="listbox" aria-label={t("general.pick_app")}>
          {#each shown as a (a.bundle_id)}
            <button type="button" class="option" class:on={a.bundle_id === chosen?.bundle_id} role="option" aria-selected={a.bundle_id === chosen?.bundle_id} onclick={() => pick(a.bundle_id)}>
              <span class="name">{a.name}</span>
              <span class="id">{a.bundle_id}</span>
            </button>
          {:else}
            <p class="none">{t(apps.length === 0 ? "general.no_apps" : "general.no_app_match")}</p>
          {/each}
        </div>
      </div>
    {/if}
  </div>
  <input class="add-input" type="text" placeholder="com.example.app" aria-label={t("general.bundle_id")} bind:value onkeydown={(e) => { if (e.key === "Enter" && !isImeKey(e)) add(); }} />
  <button type="button" class="btn" disabled={!value.trim()} onclick={add}><Icon name="plus" size={14} />{t("common.add")}</button>
</Row>

<style>
  /* The picker gives way first, so the text box keeps the width of the other add rows. */
  .apps { position: relative; width: 170px; flex: 0 4 auto; min-width: 0; }
  .current { display: flex; align-items: center; gap: 8px; width: 100%; height: var(--control); padding: 0 10px; border-radius: var(--radius); border: 1px solid var(--line-strong); background: var(--surface); color: var(--ink); font-size: var(--text-control); cursor: default; }
  .current:hover, .current[aria-expanded="true"] { background: var(--sunken); }
  .current.empty { color: var(--ink-2); }
  .current :global(svg) { flex: none; margin-left: auto; color: var(--ink-3); }
  .name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .menu { position: absolute; top: calc(100% + 6px); left: 0; z-index: 20; display: flex; flex-direction: column; width: 340px; max-width: calc(100vw - 2 * var(--page-x) - 248px); max-height: 300px; padding: 6px; border-radius: 14px; border: 1px solid var(--line); background: var(--surface); box-shadow: var(--shadow); }
  .menu.upwards { top: auto; bottom: calc(100% + 6px); }
  .menu input { flex: none; width: 100%; margin-bottom: 4px; }
  .options { overflow-y: auto; min-height: 0; }
  .option { display: block; width: 100%; padding: 7px 12px; border: 0; border-radius: 9px; background: none; color: var(--ink); text-align: left; font-size: var(--text-control); cursor: default; }
  .option:hover, .option:focus-visible { background: var(--sunken); }
  .option.on { background: var(--accent-soft); }
  .option .name { display: block; }
  .id { display: block; color: var(--ink-2); font-size: var(--text-xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .none { margin: 0; padding: 12px; color: var(--ink-2); font-size: var(--text-sm); }
  .add-input { margin-left: auto; }
</style>
