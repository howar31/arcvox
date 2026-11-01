<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  /** One line of a dropdown's list, ticked when it is the current choice. */
  let { selected = false, disabled = false, onclick, children }: {
    selected?: boolean;
    disabled?: boolean;
    onclick: () => void;
    children: Snippet;
  } = $props();
</script>

<button type="button" class="option" class:on={selected} role="option" aria-selected={selected} {disabled} {onclick}>
  <span class="tick">{#if selected}<Icon name="check" size={14} />{/if}</span>
  <span class="text">{@render children()}</span>
</button>

<style>
  .option { display: flex; align-items: flex-start; gap: 8px; width: 100%; padding: 7px 12px 7px 8px; border: 0; border-radius: 9px; background: none; color: var(--ink); text-align: left; font-size: var(--text-control); cursor: default; }
  .option:hover:not(:disabled), .option:focus-visible { background: var(--sunken); }
  .option:disabled { color: var(--ink-3); }
  .option.on { background: var(--accent-soft); }
  .tick { flex: none; width: 16px; height: 22px; display: grid; place-items: center; color: var(--accent); }
  .text { min-width: 0; flex: 1; overflow-wrap: anywhere; }
</style>
