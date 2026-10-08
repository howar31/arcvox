<script lang="ts">
  import type { Snippet } from "svelte";
  import { guard } from "../guard.svelte";
  import { go, type Page } from "../route";

  type Crumb = { label: string; page: Page; arg?: string | null };
  let { title, description, crumb, actions }: {
    title: string;
    /** A line under the title: text, or content with a link in it. */
    description?: string | Snippet;
    /** The page above this one, or the pages above it from the top down. */
    crumb?: Crumb | Crumb[];
    actions?: Snippet;
  } = $props();
  const crumbs = $derived(crumb === undefined ? [] : Array.isArray(crumb) ? crumb : [crumb]);
</script>

{#if crumbs.length}
  <nav class="crumb">
    {#each crumbs as c (c.page + (c.arg ?? ""))}
      <button type="button" onclick={() => guard.request(() => go(c.page, c.arg))}>{c.label}</button>
      <span aria-hidden="true">›</span>
    {/each}
    <span class="here">{title}</span>
  </nav>
{/if}
<header class="head">
  <div class="grow">
    <h1>{title}</h1>
    {#if typeof description === "string"}<p>{description}</p>{:else if description}<p>{@render description()}</p>{/if}
  </div>
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</header>

<style>
  .head { display: flex; align-items: flex-end; gap: 16px; margin-bottom: 22px; }
  .grow { flex: 1; min-width: 0; }
  h1 { font: 700 var(--text-title)/1.2 var(--font-round); margin: 0; letter-spacing: .01em; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  p { margin: 6px 0 0; color: var(--ink-2); }
  .actions { display: flex; align-items: center; gap: 8px; flex: none; }
  .crumb { display: flex; align-items: center; gap: 6px; color: var(--ink-2); font-size: var(--text-sm); margin-bottom: 6px; min-width: 0; }
  .crumb button { border: 0; background: none; padding: 0; color: var(--accent); cursor: default; }
  .crumb button:hover { color: var(--ink); }
  .here { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
