<script lang="ts">
  /** A result: a headline, the service's own words under it (selectable), and its full answer folded under them when there is one. */
  import { t } from "../i18n";
  import Fold from "./Fold.svelte";
  let { text, tone = "", body = null, detail = null }: {
    text: string;
    tone?: "ok" | "bad" | "";
    /** What the service said, on a line of its own. */
    body?: string | null;
    /** The service's answer as it came. */
    detail?: string | null;
  } = $props();
  let open = $state(false);
</script>

{#snippet toggle()}
  {#if detail}<button type="button" class="toggle" aria-expanded={open} onclick={() => (open = !open)}>{t(open ? "engines.raw_hide" : "engines.raw_show")}</button>{/if}
{/snippet}

<span class="answer">
  <span class="head {tone}">{text}{#if !body}{@render toggle()}{/if}</span>
  {#if body}<span class="body"><span class="sel">{body}</span>{@render toggle()}</span>{/if}
</span>
{#if detail && open}
  <Fold><pre class="raw sel">{detail}</pre></Fold>
{/if}

<style>
  .answer { display: block; overflow-wrap: anywhere; }
  .head { display: block; }
  .head.ok { color: var(--ok); }
  .head.bad { color: var(--live); }
  .body { display: block; margin-top: 2px; color: var(--ink-2); }
  .sel { -webkit-user-select: text; user-select: text; cursor: text; }
  .toggle { margin-left: 8px; padding: 0; border: 0; background: none; color: var(--accent); font: inherit; font-size: var(--text-xs); cursor: default; }
  .raw { margin: 6px 0 0; padding: 8px 10px; max-height: 160px; overflow: auto; border-radius: 8px; background: var(--sunken); color: var(--ink-2); font: var(--text-xs)/1.5 ui-monospace, Menlo, monospace; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
