<script lang="ts" module>
  export interface MoreItem { label: string; onclick: () => void; danger?: boolean; disabled?: boolean; tip?: string; confirm?: string; divider?: boolean }
</script>

<script lang="ts">
  /**
   * An icon button that opens a short menu of a row's less used actions, under the button and aligned
   * to its right edge. An item with `confirm` asks again: the first press turns it into that question
   * for 4 seconds, the second does it.
   */
  import { tick } from "svelte";
  import { isImeKey } from "../ime";
  import { placeMenu, type Placement } from "../popover";
  import { tip } from "../tip";
  import Icon from "./Icon.svelte";

  let { label, items }: { label: string; items: MoreItem[] } = $props();

  const WIDTH = 200;
  let open = $state(false);
  let root: HTMLDivElement | undefined = $state();
  let trigger: HTMLButtonElement | undefined = $state();
  let menu: HTMLDivElement | undefined = $state();
  let place = $state<Placement | null>(null);
  let armed = $state<number | null>(null);
  let armTimer: ReturnType<typeof setTimeout> | undefined;

  function anchor() {
    const r = trigger!.getBoundingClientRect();
    // The menu ends where the button ends.
    return { top: r.top, bottom: r.bottom, left: Math.max(8, r.right - WIDTH), width: WIDTH };
  }
  async function show() {
    open = true;
    armed = null;
    place = placeMenu(anchor(), 0, { width: window.innerWidth, height: window.innerHeight });
    await tick();
    if (menu) place = placeMenu(anchor(), menu.offsetHeight, { width: window.innerWidth, height: window.innerHeight });
    menu?.querySelector<HTMLButtonElement>("button:not([disabled])")?.focus();
  }
  function hide(refocus = true) {
    if (!open) return;
    open = false;
    clearTimeout(armTimer);
    armed = null;
    if (refocus) trigger?.focus();
  }
  function choose(i: number) {
    const item = items[i];
    if (item.disabled) return;
    if (item.confirm && armed !== i) {
      armed = i;
      clearTimeout(armTimer);
      armTimer = setTimeout(() => (armed = null), 4000);
      return;
    }
    hide();
    item.onclick();
  }
  function onkeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape" && !isImeKey(e)) { e.preventDefault(); e.stopPropagation(); hide(); return; }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const all = [...(menu?.querySelectorAll<HTMLButtonElement>("button:not([disabled])") ?? [])];
    const at = all.indexOf(document.activeElement as HTMLButtonElement);
    all[e.key === "ArrowDown" ? (at + 1) % all.length : at <= 0 ? all.length - 1 : at - 1]?.focus();
  }

  $effect(() => {
    if (!open) return;
    const outside = (e: Event) => { if (root && !root.contains(e.target as Node)) hide(false); };
    const away = () => hide(false);
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("scroll", away, true);
    window.addEventListener("blur", away);
    window.addEventListener("resize", away);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("scroll", away, true);
      window.removeEventListener("blur", away);
      window.removeEventListener("resize", away);
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="more" bind:this={root} {onkeydown}>
  <button type="button" class="btn icon-btn" bind:this={trigger} aria-label={label} aria-haspopup="menu" aria-expanded={open} use:tip={open ? undefined : label} onclick={() => (open ? hide() : show())}><Icon name="more" /></button>
  {#if open}
    <div class="menu" role="menu" bind:this={menu}
      style:top={place?.top != null ? `${place.top}px` : null} style:bottom={place?.bottom != null ? `${place.bottom}px` : null}
      style:left={place ? `${place.left}px` : null} style:width={`${WIDTH}px`}>
      {#each items as item, i (item.label)}
        {#if item.divider}<hr />{/if}
        <button type="button" role="menuitem" class="item" class:danger={item.danger} class:ask={armed === i} disabled={item.disabled} use:tip={item.tip} onclick={() => choose(i)}>{armed === i ? item.confirm : item.label}</button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .more { position: relative; display: inline-flex; }
  .icon-btn { width: 32px; height: 32px; padding: 0; justify-content: center; color: var(--ink-2); }
  .icon-btn:hover, .icon-btn[aria-expanded="true"] { color: var(--ink); }
  .menu { position: fixed; z-index: 60; display: flex; flex-direction: column; padding: 6px; border-radius: 14px; border: 1px solid var(--line); background: var(--surface); box-shadow: var(--shadow); }
  .item { width: 100%; padding: 7px 10px; border: 0; border-radius: 9px; background: none; color: var(--ink); font-size: var(--text-control); text-align: left; cursor: default; }
  .item:hover:not(:disabled), .item:focus-visible { background: var(--sunken); }
  .item:disabled { color: var(--ink-3); }
  .item.danger { color: var(--live); }
  .item.ask { color: var(--live); font-weight: 600; }
  hr { margin: 5px 4px; border: 0; border-top: 1px solid var(--line); }
</style>
