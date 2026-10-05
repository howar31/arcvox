<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "../i18n";
  let { title, section, help, actions, children, onreset }: {
    title?: string;
    /** The name a link uses to bring this group into view. */
    section?: string;
    help?: string;
    actions?: Snippet;
    children: Snippet;
    /** Puts the group's settings back to their defaults; set while one of them differs. */
    onreset?: () => void;
  } = $props();
</script>

<section class="section" class:untitled={!title} data-section={section}>
  {#if title && actions}
    <div class="top"><h2>{title}</h2><div class="actions">{@render actions()}</div></div>
  {:else if title && onreset}
    <div class="top"><h2>{title}</h2><button type="button" class="reset" data-tip={t("common.reset_group")} onclick={onreset}>{t("common.reset_default")}</button></div>
  {:else if title}<h2>{title}</h2>{/if}
  <div class="group">{@render children()}</div>
  {#if help}<p class="help">{help}</p>{/if}
</section>

<style>
  h2 { font: 600 var(--text-sm)/1 var(--font); color: var(--ink-2); margin: var(--gap-group) 4px 12px; }
  /* The actions hang into the title's top margin, so the title stays where it is without them. */
  .top { display: flex; align-items: flex-end; justify-content: space-between; gap: 12px; }
  .top .actions { display: flex; align-items: center; gap: 8px; margin: 0 0 8px; }
  /* Text alone, in the title's line: the title stays where it is when this appears. */
  .reset { border: 0; background: none; padding: 6px 4px; margin: 0 0 6px; font: var(--text-sm)/1 var(--font); color: var(--ink-3); cursor: default; }
  .reset { position: relative; }
  .reset:hover, .reset:focus-visible { color: var(--ink); }
  .reset:hover::after, .reset:focus-visible::after { content: attr(data-tip); position: absolute; right: calc(100% + 6px); top: 50%; transform: translateY(-50%); z-index: 6; padding: 5px 9px; border-radius: 8px; background: var(--ink); color: var(--page); font-size: var(--text-xs); white-space: nowrap; pointer-events: none; }
  .untitled { margin-top: 22px; }
  .section[data-section] { scroll-margin-top: 20px; }
  /* A link led here: the group is outlined, then the outline fades. */
  .section:global([data-arrived]) .group { animation: arrived 1.6s ease-out; }
  @keyframes arrived {
    0%, 55% { box-shadow: 0 0 0 3px var(--accent-soft), 0 0 0 1px var(--accent); }
    100% { box-shadow: 0 0 0 3px transparent, 0 0 0 1px transparent; }
  }
  .group { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); }
</style>
