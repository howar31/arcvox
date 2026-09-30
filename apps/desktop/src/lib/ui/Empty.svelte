<script lang="ts">
  import type { Snippet } from "svelte";
  let { text, action }: { text: string; action?: Snippet } = $props();
</script>

<div class="empty">
  <p>{text}</p>
  {#if action}<div class="action">{@render action()}</div>{/if}
</div>

<style>
  .empty { display: flex; flex-direction: column; align-items: center; gap: 12px; padding: 24px 16px; text-align: center; color: var(--ink-2); }
  p { margin: 0; }
</style>
