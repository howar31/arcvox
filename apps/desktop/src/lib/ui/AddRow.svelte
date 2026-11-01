<script lang="ts">
  import type { Snippet } from "svelte";

  /** The last row of a list in a card: what adds to the list. */
  let { children }: { children: Snippet } = $props();
</script>

<div class="add">{@render children()}</div>

<style>
  .add { display: flex; flex-wrap: wrap; gap: 10px; padding: 14px var(--row-x); border-top: 1px solid var(--line); }
  .add:first-child { border-top: 0; }
</style>
