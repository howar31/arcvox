<script lang="ts">
  import type { Snippet } from "svelte";
  import { guard } from "../guard.svelte";
  import { go, type Page } from "../route";

  let { title, description, crumb, actions }: {
    title: string;
    description?: string;
    crumb?: { label: string; page: Page; arg?: string | null };
    actions?: Snippet;
  } = $props();
</script>

{#if crumb}
  <nav class="crumb">
    <button type="button" onclick={() => guard.request(() => go(crumb.page, crumb.arg))}>{crumb.label}</button>
    <span aria-hidden="true">›</span>
    <span class="here">{title}</span>
  </nav>
{/if}
<header class="head">
  <div class="grow">
    <h1>{title}</h1>
    {#if description}<p>{description}</p>{/if}
  </div>
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</header>

<style>
  .head { display: flex; align-items: flex-end; gap: 16px; margin-bottom: 20px; }
  .grow { flex: 1; min-width: 0; }
  h1 { font: 600 21px/1.2 var(--font-round); margin: 0; letter-spacing: .01em; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  p { margin: 4px 0 0; color: var(--ink-2); }
  .actions { display: flex; align-items: center; gap: 8px; flex: none; }
  .crumb { display: flex; align-items: center; gap: 6px; color: var(--ink-2); font-size: 12px; margin-bottom: 6px; min-width: 0; }
  .crumb button { border: 0; background: none; padding: 0; color: var(--accent); cursor: default; }
  .crumb button:hover { text-decoration: underline; }
  .here { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
