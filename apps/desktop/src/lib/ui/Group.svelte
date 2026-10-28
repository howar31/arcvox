<script lang="ts">
  import { tip } from "../tip";
  import type { Snippet } from "svelte";
  import { t } from "../i18n";
  import InfoTip from "./InfoTip.svelte";
  let { title, section, help, info, actions, children, onreset }: {
    title?: string;
    /** The name a link uses to bring this group into view. */
    section?: string;
    help?: string;
    /** The second layer of the help, on the card of the ⓘ beside the title (at the end of the help line when there is no title). */
    info?: string | Snippet;
    actions?: Snippet;
    children: Snippet;
    /** Puts the group's settings back to their defaults; set while one of them differs. */
    onreset?: () => void;
  } = $props();

</script>

<section class="section" class:untitled={!title} data-section={section}>
  <!-- One title row whether or not it carries actions or the reset: the title's margins are the same
       either way, so the group does not move when the reset appears. -->
  {#if title}
    <div class="top">
      <h2>{title}{#if info}<InfoTip {info} />{/if}</h2>
      {#if actions}<div class="actions">{@render actions()}</div>{:else if onreset}<button type="button" class="reset" use:tip={t("common.reset_group")} onclick={onreset}>{t("common.reset_default")}</button>{/if}
    </div>
  {/if}
  <div class="group">{@render children()}</div>
  {#if help || (info && !title)}<p class="help">{help ?? ""}{#if info && !title}<InfoTip {info} />{/if}</p>{/if}
</section>

<style>
  h2 { display: flex; align-items: center; gap: 6px; font: 600 var(--text-sm)/1 var(--font); color: var(--ink-2); margin: var(--gap-group) 4px 10px; }
  /* The actions hang into the title's top margin, so the title stays where it is without them. */
  .top { display: flex; align-items: flex-end; justify-content: space-between; gap: 12px; }
  .top .actions { display: flex; align-items: center; gap: 8px; margin: 0 0 8px; }
  /* Text alone, in the title's line: the title stays where it is when this appears. */
  .reset { border: 0; background: none; padding: 6px 4px; margin: 0 0 6px; font: var(--text-sm)/1 var(--font); color: var(--ink-3); cursor: default; }
  .reset { position: relative; }
  .reset:hover, .reset:focus-visible { color: var(--ink); }
  .untitled { margin-top: 22px; }
  .section[data-section] { scroll-margin-top: 20px; }
  /* A link led here: the group is outlined, then the outline fades. */
  .section:global([data-arrived]) .group { animation: arrived 1.6s ease-out; }
  @keyframes arrived {
    0%, 55% { box-shadow: 0 0 0 3px var(--accent-soft), 0 0 0 1px var(--accent); }
    100% { box-shadow: 0 0 0 3px transparent, 0 0 0 1px transparent; }
  }
  .group { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); }
  .help :global(.info-btn) { margin-left: 2px; }
</style>
