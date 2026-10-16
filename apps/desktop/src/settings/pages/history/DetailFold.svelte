<script lang="ts">
  /** A block of an expanded History row that opens on request: a title row with a count, and the content below it. */
  import type { Snippet } from "svelte";
  import Fold from "../../../lib/ui/Fold.svelte";
  import Icon from "../../../lib/ui/Icon.svelte";

  let { title, small, right = "", open, ontoggle, children }: {
    title: string;
    /** The count or the state, beside the title. */
    small: string;
    /** What matters while the block is closed, at the row's right. */
    right?: string;
    open: boolean;
    ontoggle: (open: boolean) => void;
    children: Snippet;
  } = $props();
</script>

<div class="block" class:open>
  <button type="button" class="head" aria-expanded={open} onclick={() => ontoggle(!open)}>
    <span class="chev"><Icon name="chevron-right" size={14} /></span>
    <span class="title">{title}</span>
    <small>{small}</small>
    {#if right && !open}<span class="right">{right}</span>{/if}
  </button>
  {#if open}<Fold><div class="body">{@render children()}</div></Fold>{/if}
</div>

<style>
  .block { margin-top: 12px; border: 1px solid var(--line); border-radius: 14px; background: var(--surface); }
  .head { display: flex; align-items: center; gap: 10px; width: 100%; min-height: 44px; padding: 0 16px; border: 0; border-radius: 14px; background: none; color: var(--ink); text-align: left; font-size: var(--text-control); cursor: default; }
  .head:hover { background: var(--sunken); }
  .open .head { border-radius: 14px 14px 0 0; }
  .head:focus-visible { outline-offset: -2px; }
  .chev { display: inline-grid; place-items: center; width: 14px; color: var(--ink-3); transition: transform .2s; }
  .open .chev { transform: rotate(90deg); }
  .title { font-weight: 600; }
  small { color: var(--ink-2); font-size: var(--text-sm); }
  .right { margin-left: auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--ink-2); font-size: var(--text-sm); }
  .body { padding: 2px 16px 14px; border-top: 1px solid var(--line); }
</style>
