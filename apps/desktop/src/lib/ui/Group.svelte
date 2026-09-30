<script lang="ts">
  import type { Snippet } from "svelte";
  let { title, help, children }: { title?: string; help?: string; children: Snippet } = $props();
</script>

<section class="section" class:untitled={!title}>
  {#if title}<h2>{title}</h2>{/if}
  <div class="group">{@render children()}</div>
  {#if help}<p class="help">{help}</p>{/if}
</section>

<style>
  h2 { font: 600 12px/1 var(--font); color: var(--ink-2); margin: 22px 2px 8px; }
  .untitled { margin-top: 16px; }
  .group { background: var(--surface); border: 1px solid var(--line); border-radius: 10px; }
</style>
