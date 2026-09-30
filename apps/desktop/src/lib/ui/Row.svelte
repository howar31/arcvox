<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  let { label, help, leading, children, onclick }: {
    label?: string;
    help?: string;
    leading?: Snippet;
    children?: Snippet;
    /** Makes the whole row a button that ends in a chevron. */
    onclick?: () => void;
  } = $props();
</script>

{#snippet body()}
  {#if leading}{@render leading()}{/if}
  {#if label || help}
    <span class="label">
      {#if label}<span class="text">{label}</span>{/if}
      {#if help}<small>{help}</small>{/if}
    </span>
  {/if}
  {#if children}{@render children()}{/if}
{/snippet}

{#if onclick}
  <button type="button" class="item link" {onclick}>
    {@render body()}
    <span class="chev"><Icon name="chevron-right" /></span>
  </button>
{:else}
  <div class="item">{@render body()}</div>
{/if}

<style>
  .item { display: flex; align-items: center; gap: 12px; min-height: 44px; padding: 8px 14px; width: 100%; }
  .item + :global(.item) { border-top: 1px solid var(--line); }
  .label { display: block; flex: 1; min-width: 0; }
  .text { display: block; }
  .label small { display: block; color: var(--ink-2); font-size: 12px; overflow-wrap: anywhere; }
  .link { border: 0; border-radius: 0; background: none; text-align: left; cursor: default; }
  .link:hover { background: var(--sunken); }
  .link:first-child { border-top-left-radius: 9px; border-top-right-radius: 9px; }
  .link:last-child { border-bottom-left-radius: 9px; border-bottom-right-radius: 9px; }
  .link:focus-visible { outline-offset: -2px; }
  .chev { display: flex; color: var(--ink-3); }
  /* Line a trailing quiet button's text up with plain values in the column. */
  .item > :global(.btn.quiet:last-child) { margin-right: -6px; }
</style>
