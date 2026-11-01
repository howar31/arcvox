<script lang="ts">
  import { tick, type Snippet } from "svelte";
  import { t } from "../i18n";
  import { isImeKey } from "../ime";
  import { placeMenu, type Placement } from "../popover";
  import Icon from "./Icon.svelte";

  /**
   * A button that opens a list in a menu fixed in the window: the one dropdown of the app. The list keeps
   * its place by the button while the page scrolls. `button` is what the closed control shows, `list` the
   * options (`MenuOption`), `foot` an optional last line; the last two get the function that closes the menu.
   */
  let { label, button, list, foot, search = false, query = $bindable(""), placeholder = "", disabled = false, large = false, minWidth = 0 }: {
    /** Accessible name of the control. */
    label: string;
    button: Snippet;
    list: Snippet<[() => void]>;
    foot?: Snippet<[() => void]>;
    /** Shows a search field over the list; `query` holds what is typed. */
    search?: boolean;
    query?: string;
    placeholder?: string;
    disabled?: boolean;
    /** The taller button, for a control that stands alone. */
    large?: boolean;
    /** Widens the menu of a narrow button. */
    minWidth?: number;
  } = $props();

  let open = $state(false);
  let root: HTMLDivElement | undefined = $state();
  let trigger: HTMLButtonElement | undefined = $state();
  let menu: HTMLDivElement | undefined = $state();
  // Where the open menu sits in the window; null while its natural height is being measured.
  let place = $state<Placement | null>(null);
  let measuring = $state(false);
  // The menu's natural height, measured when it opens.
  let need = 0;

  const view = () => ({ width: window.innerWidth, height: window.innerHeight });
  const options = () => [...(menu?.querySelectorAll<HTMLButtonElement>("[role=option]:not([disabled])") ?? [])];

  async function show() {
    if (disabled) return;
    query = "";
    open = true;
    // The menu is measured at its final width before it is placed.
    const anchor = trigger!.getBoundingClientRect();
    place = { ...placeMenu(anchor, 0, view(), minWidth), top: null, bottom: null };
    measuring = true;
    await tick();
    if (menu) { need = menu.offsetHeight; place = placeMenu(anchor, need, view(), minWidth); }
    measuring = false;
    // A hidden menu takes no focus.
    await tick();
    const field = menu?.querySelector("input");
    if (field) field.focus();
    else (options().find((o) => o.getAttribute("aria-selected") === "true") ?? options()[0])?.focus();
    menu?.querySelector("[aria-selected=true]")?.scrollIntoView({ block: "nearest" });
  }
  function hide(refocus = true) {
    if (!open) return;
    open = false;
    if (refocus) trigger?.focus();
  }
  /** A scroll elsewhere moves the button, and the menu goes along. */
  function onscroll(e: Event) {
    if (!trigger || (menu && e.target instanceof Node && menu.contains(e.target))) return;
    place = placeMenu(trigger.getBoundingClientRect(), need, view(), minWidth);
  }
  function onkeydown(e: KeyboardEvent) {
    if (!open) return;
    // Escape during composition cancels the input method, not the menu.
    if (e.key === "Escape") { if (!isImeKey(e)) { e.preventDefault(); e.stopPropagation(); hide(); } return; }
    const all = options();
    if (e.key === "Enter" && document.activeElement instanceof HTMLInputElement) {
      if (!e.isComposing) { e.preventDefault(); all[0]?.click(); }
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    if (all.length === 0) return;
    const at = all.indexOf(document.activeElement as HTMLButtonElement);
    const next = e.key === "ArrowDown" ? (at + 1) % all.length : at <= 0 ? all.length - 1 : at - 1;
    all[next].focus();
  }
  function onbuttonkey(e: KeyboardEvent) {
    if (open || (e.key !== "ArrowDown" && e.key !== "ArrowUp")) return;
    e.preventDefault();
    show();
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

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="dropdown" bind:this={root} {onkeydown}>
  <button type="button" class="current" class:large bind:this={trigger} {disabled} aria-haspopup="listbox" aria-expanded={open} aria-label={label} onclick={() => (open ? hide() : show())} onkeydown={onbuttonkey}>
    {@render button()}
    <Icon name="chevron-down" size={14} />
  </button>
  {#if open}
    <div
      class="menu" bind:this={menu}
      style:top={place?.top != null ? `${place.top}px` : null} style:bottom={place?.bottom != null ? `${place.bottom}px` : null}
      style:left={place ? `${place.left}px` : null} style:width={place ? `${place.width}px` : null} style:max-height={place?.maxHeight != null ? `${place.maxHeight}px` : null}
      style:visibility={measuring ? "hidden" : null}
    >
      {#if search}<input type="search" bind:value={query} placeholder={placeholder || t("common.search")} aria-label={placeholder || t("common.search")} />{/if}
      <div class="options" role="listbox" aria-label={label}>{@render list(hide)}</div>
      {#if foot}<div class="foot">{@render foot(hide)}</div>{/if}
    </div>
  {/if}
</div>

<style>
  .dropdown { position: relative; min-width: 0; }
  .current { display: flex; align-items: center; gap: 10px; width: 100%; height: var(--control); padding: 0 12px; border-radius: var(--radius); border: 1px solid var(--line-strong); background: var(--surface); color: var(--ink); font-size: var(--text-control); font-weight: 500; text-align: left; cursor: default; }
  .current.large { height: 44px; padding: 0 14px; border-radius: 12px; font-size: var(--text); font-weight: 600; }
  .current:hover:not(:disabled), .current[aria-expanded="true"] { background: var(--sunken); }
  .current:disabled { opacity: .55; }
  .current :global(svg) { flex: none; color: var(--ink-3); }
  .menu { position: fixed; z-index: 60; display: flex; flex-direction: column; padding: 6px; border-radius: 14px; border: 1px solid var(--line); background: var(--surface); box-shadow: var(--shadow); }
  .menu input { flex: none; width: 100%; margin-bottom: 4px; }
  .options { overflow-y: auto; overscroll-behavior: contain; min-height: 0; }
  .foot { flex: none; display: flex; align-items: center; justify-content: space-between; gap: 8px; margin: 6px -6px -6px; padding: 4px 6px 4px 14px; border-top: 1px solid var(--line); background: var(--sunken); border-radius: 0 0 14px 14px; color: var(--ink-2); font-size: var(--text-sm); }
</style>
