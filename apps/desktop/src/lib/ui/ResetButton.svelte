<script lang="ts">
  import { tip } from "../tip";
  import Icon from "./Icon.svelte";
  import type { Reset } from "../reset";

  /** Shown only while the setting differs from its default. */
  let { reset }: { reset?: Reset } = $props();
</script>

{#if reset}
  <button type="button" class="reset" aria-label={reset.hint} use:tip={reset.hint} onclick={reset.run}><Icon name="undo" /></button>
{/if}

<style>
  .reset { position: relative; display: grid; place-items: center; flex: none; width: 28px; height: 28px; padding: 0; border: 0; border-radius: 8px; background: none; color: var(--ink-3); cursor: default; }
  .reset:hover, .reset:focus-visible { color: var(--ink); background: var(--sunken); }
</style>
