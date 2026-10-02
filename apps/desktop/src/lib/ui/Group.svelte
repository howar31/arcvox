<script lang="ts">
  import type { Snippet } from "svelte";
  let { title, help, actions, children }: { title?: string; help?: string; actions?: Snippet; children: Snippet } = $props();
</script>

<section class="section" class:untitled={!title}>
  {#if title && actions}
    <div class="top"><h2>{title}</h2><div class="actions">{@render actions()}</div></div>
  {:else if title}<h2>{title}</h2>{/if}
  <div class="group">{@render children()}</div>
  {#if help}<p class="help">{help}</p>{/if}
</section>

<style>
  h2 { font: 600 var(--text-sm)/1 var(--font); color: var(--ink-2); margin: var(--gap-group) 4px 12px; }
  /* The actions hang into the title's top margin, so the title stays where it is without them. */
  .top { display: flex; align-items: flex-end; justify-content: space-between; gap: 12px; }
  .top .actions { display: flex; align-items: center; gap: 8px; margin: 0 0 8px; }
  .untitled { margin-top: 22px; }
  .group { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); }
</style>
