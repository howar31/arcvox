<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import ResetButton from "./ResetButton.svelte";
  import type { Reset } from "../reset";

  let { label, tag, help, leading, children, onclick, external = false, reset }: {
    label?: string;
    /** A tag beside the label, where every page puts the tag of a name. */
    tag?: Snippet;
    /** A line under the label: text, or content with a link in it. */
    help?: string | Snippet;
    leading?: Snippet;
    children?: Snippet;
    /** Makes the whole row a button that ends in a chevron. */
    onclick?: () => void;
    /** The row opens something outside the app: it ends in the external mark instead of the chevron. */
    external?: boolean;
    /** Set while the row's value differs from its default. */
    reset?: Reset;
  } = $props();
</script>

{#snippet body()}
  {#if leading}{@render leading()}{/if}
  {#if label || help}
    <span class="label">
      {#if label}<span class="text" class:tagged={tag}>{label}{#if tag}{@render tag()}{/if}</span>{/if}
      {#if typeof help === "string"}<small>{help}</small>{:else if help}<small>{@render help()}</small>{/if}
    </span>
  {/if}
  <ResetButton {reset} />
  {#if children}{@render children()}{/if}
{/snippet}

{#if onclick}
  <button type="button" class="item link" {onclick}>
    {@render body()}
    <span class="chev"><Icon name={external ? "external" : "chevron-right"} /></span>
  </button>
{:else}
  <div class="item">{@render body()}</div>
{/if}

<style>
  .item { display: flex; align-items: center; gap: 16px; min-height: var(--row-min); padding: var(--row-y) var(--row-x); width: 100%; }
  .item + :global(.item), :global(.fold) + .item, :global(.item + .fold) > .item:first-child, :global(.fold + .fold) > .item:first-child { border-top: 1px solid var(--line); }
  .label { display: block; flex: 1; min-width: 0; }
  .text { display: block; font-weight: 600; }
  .text.tagged { display: flex; align-items: center; gap: 8px; }
  .label small { display: block; color: var(--ink-2); font-size: var(--text-sm); margin-top: 2px; overflow-wrap: anywhere; }
  .link { border: 0; border-radius: 0; background: none; text-align: left; cursor: default; }
  .link:hover { background: var(--sunken); }
  .link:first-child { border-top-left-radius: calc(var(--radius-group) - 1px); border-top-right-radius: calc(var(--radius-group) - 1px); }
  .link:last-child { border-bottom-left-radius: calc(var(--radius-group) - 1px); border-bottom-right-radius: calc(var(--radius-group) - 1px); }
  .link:focus-visible { outline-offset: -2px; }
  .chev { display: flex; color: var(--ink-3); }
  /* Line a trailing quiet button's text up with plain values in the column. */
  .item > :global(.btn.quiet:last-child) { margin-right: -8px; }
</style>
