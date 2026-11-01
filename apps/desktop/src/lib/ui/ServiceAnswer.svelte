<script lang="ts">
  /** A result line that can be selected and copied, with the service's own answer folded under it when there is one. */
  import { t } from "../i18n";
  import Fold from "./Fold.svelte";
  let { text, tone = "", detail = null }: {
    text: string;
    tone?: "ok" | "bad" | "";
    /** The service's answer as it came. */
    detail?: string | null;
  } = $props();
  let open = $state(false);
</script>

<span class="answer {tone}">
  <span class="text">{text}</span>
  {#if detail}<button type="button" class="toggle" aria-expanded={open} onclick={() => (open = !open)}>{t(open ? "engines.raw_hide" : "engines.raw_show")}</button>{/if}
</span>
{#if detail && open}
  <Fold><pre class="raw">{detail}</pre></Fold>
{/if}

<style>
  .answer { display: block; overflow-wrap: anywhere; }
  .answer.ok { color: var(--ok); }
  .answer.bad { color: var(--live); }
  .text, .raw { -webkit-user-select: text; user-select: text; cursor: text; }
  .toggle { margin-left: 8px; padding: 0; border: 0; background: none; color: var(--accent); font: inherit; font-size: var(--text-xs); cursor: default; }
  .raw { margin: 6px 0 0; padding: 8px 10px; max-height: 160px; overflow: auto; border-radius: 8px; background: var(--sunken); color: var(--ink-2); font: var(--text-xs)/1.5 ui-monospace, Menlo, monospace; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
