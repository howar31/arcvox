<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { Reset } from "../reset";

  /** Shown only while the setting differs from its default. */
  let { reset }: { reset?: Reset } = $props();
</script>

{#if reset}
  <button type="button" class="reset" aria-label={reset.hint} data-tip={reset.hint} onclick={reset.run}><Icon name="undo" /></button>
{/if}

<style>
  .reset { position: relative; display: grid; place-items: center; flex: none; width: 28px; height: 28px; padding: 0; border: 0; border-radius: 8px; background: none; color: var(--ink-3); cursor: default; }
  .reset:hover, .reset:focus-visible { color: var(--ink); background: var(--sunken); }
  /* Says what the default is, at once: the system's own tooltip waits a second. */
  .reset:hover::after, .reset:focus-visible::after { content: attr(data-tip); position: absolute; right: calc(100% + 6px); top: 50%; transform: translateY(-50%); z-index: 6; padding: 5px 9px; border-radius: 8px; background: var(--ink); color: var(--page); font-size: var(--text-xs); white-space: nowrap; pointer-events: none; }
</style>
