<script lang="ts">
  import { onMount } from "svelte";
  import { openSettingsWindow, panelFit, widgetCopy, widgetDismiss } from "../lib/api";
  import { t } from "../lib/i18n";
  import { panelView, type PanelAction } from "../lib/widgetView";
  import { WidgetFeed } from "../lib/widgetEvents.svelte";
  import Icon from "../lib/ui/Icon.svelte";

  const feed = new WidgetFeed();
  let box = $state<HTMLElement>();

  onMount(() => feed.listen());

  const view = $derived.by(() => {
    void feed.language;
    void feed.scale;
    return panelView(feed.state, feed.message, feed.transcript, feed.hint);
  });

  // The window takes the size of the content; without content it is hidden.
  $effect(() => {
    if (!view || !box) {
      panelFit(null, null).catch(() => {});
      return;
    }
    const r = box.getBoundingClientRect();
    panelFit(r.width, r.height).catch(() => {});
  });

  const LABELS: Record<PanelAction, string> = { copy: "widget.copy", copy_detail: "panel.copy_detail", open_engines: "panel.open_settings", open_history: "panel.open_history" };

  function run(action: PanelAction) {
    if (action === "copy" && view?.result) widgetCopy(view.result).catch(() => {});
    else if (action === "copy_detail" && view?.detail) widgetCopy(view.detail).catch(() => {});
    else if (action === "open_engines" || action === "open_history") {
      openSettingsWindow(action === "open_engines" ? "engines" : "history").catch(() => {});
      dismiss();
    }
  }

  function dismiss() {
    widgetDismiss().catch(() => {});
    feed.message = null;
  }
</script>

{#if view}
  <div class="panel" bind:this={box}>
    {#if view.caption}<div class="caption">{view.caption}</div>{/if}
    {#if view.title}
      <div class="head">
        <div class="title">{view.title}</div>
        {#if view.close}<button type="button" class="close" onclick={dismiss} aria-label={t("widget.dismiss")} title={t("widget.dismiss")}><Icon name="x" size={14 * feed.scale} /></button>{/if}
      </div>
    {/if}
    {#if view.text}<div class="text">{view.text}</div>{/if}
    {#if view.body}<div class="body">{view.body}</div>{/if}
    {#if view.result}<div class="quote result">{view.result}</div>{/if}
    {#if view.detail}<div class="quote">{view.detail}</div>{/if}
    {#if view.actions.length}
      <div class="actions">
        {#each view.actions as action (action)}<button type="button" onclick={() => run(action)}>{t(LABELS[action])}</button>{/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  /* Sized by its content, whatever the size of the window at that moment. */
  .panel { position: fixed; left: 0; top: 0; width: max-content; min-width: calc(200 * var(--u)); max-width: calc(372 * var(--u)); border-radius: calc(20 * var(--u)); padding: calc(18 * var(--u)) calc(20 * var(--u)); background: var(--pill-bg); color: var(--pill-ink); box-shadow: inset 0 0 0 1px var(--pill-line); font-size: calc(13 * var(--u)); line-height: 1.6; user-select: none; -webkit-user-select: none; cursor: default; }
  .caption { font-size: calc(11 * var(--u)); color: var(--pill-ink-2); margin-bottom: calc(6 * var(--u)); }
  .head { display: flex; align-items: flex-start; gap: calc(12 * var(--u)); }
  .title { flex: 1; font-weight: 600; }
  /* About six lines, then it scrolls. */
  .text { max-height: calc(126 * var(--u)); overflow-y: auto; overflow-wrap: anywhere; }
  .body { margin-top: calc(8 * var(--u)); color: var(--pill-ink-2); white-space: pre-line; }
  .quote { margin-top: calc(14 * var(--u)); padding: calc(10 * var(--u)) calc(12 * var(--u)); border-radius: calc(10 * var(--u)); background: var(--pill-button); font-family: var(--font-mono); font-size: calc(11 * var(--u)); line-height: 1.6; color: var(--pill-ink-2); max-height: calc(108 * var(--u)); overflow-y: auto; overflow-wrap: anywhere; }
  .quote.result { font-family: inherit; font-size: calc(13 * var(--u)); color: var(--pill-ink); max-height: calc(146 * var(--u)); }
  .actions { display: flex; justify-content: flex-end; gap: calc(8 * var(--u)); margin-top: calc(16 * var(--u)); }
  button { flex: none; display: inline-flex; align-items: center; justify-content: center; height: calc(30 * var(--u)); padding: 0 calc(14 * var(--u)); border: 0; border-radius: calc(15 * var(--u)); background: var(--pill-button); color: var(--pill-ink); font: inherit; font-size: calc(12 * var(--u)); line-height: 1; cursor: default; }
  button:hover { background: var(--pill-button-hover); }
  button:focus-visible { outline-color: var(--pill-accent); }
  .close { width: calc(24 * var(--u)); height: calc(24 * var(--u)); padding: 0; margin: calc(-2 * var(--u)) calc(-6 * var(--u)) 0 0; background: none; color: var(--pill-ink-2); }
</style>
