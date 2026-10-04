<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { onPanelPlaced, openSettingsWindow, panelFit, widgetCopy, widgetDismiss, type PanelEdge } from "../lib/api";
  import { t } from "../lib/i18n";
  import { panelView, type PanelAction } from "../lib/widgetView";
  import { WidgetFeed } from "../lib/widgetEvents.svelte";
  import Icon from "../lib/ui/Icon.svelte";
  import Rewrite from "./Rewrite.svelte";

  // The panel's unit is 14/11 of the widget's.
  const feed = new WidgetFeed(14 / 11);
  let box = $state<HTMLElement>();
  /** The final text laid out on its own, to know the room it needs. */
  let ghost = $state<HTMLElement>();
  /** The size the panel keeps while one text turns into the other. */
  let held = $state<{ w: number; h: number } | null>(null);
  const still = window.matchMedia("(prefers-reduced-motion: reduce)");

  /** How long the panel takes to close; the transition below has the same length. */
  const CLOSE_MS = 170;
  /** The edge that faces the widget: the panel opens from it and closes towards it. */
  let edge = $state<PanelEdge>("bottom");
  /** The window is on screen at its place with the content it was told about. */
  let placed = $state(false);

  onMount(() => {
    const stop = feed.listen();
    const placing = onPanelPlaced((e) => { edge = e; placed = true; });
    return () => { stop(); placing.then((off) => off()); };
  });

  const view = $derived.by(() => {
    void feed.language;
    void feed.scale;
    return panelView(feed.state, feed.message, feed.transcript, feed.refined, feed.hint);
  });
  // What is drawn: the content, or the last content while the panel closes.
  let kept = $state<typeof view>(null);
  let closing: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const next = view;
    clearTimeout(closing);
    if (next) kept = next;
    else if (!untrack(() => placed) || still.matches) kept = null;
    else closing = setTimeout(() => { kept = null; }, CLOSE_MS);
  });
  // The widget leaves: its window goes too, and the next content is placed anew.
  $effect(() => {
    if (!feed.open) placed = false;
  });
  const open = $derived(placed && feed.open && view !== null);
  // One value for one pair of texts, whatever else changes meanwhile.
  const pair = $derived(view?.rewrite ? view.rewrite.from + "\u0000" + view.rewrite.to : null);

  // While the text changes the panel keeps room for both the old and the new text, so the window is resized once before and once after.
  $effect(() => {
    if (!pair || !box || !ghost || still.matches) {
      held = null;
      return;
    }
    const before = box.getBoundingClientRect(), after = ghost.getBoundingClientRect();
    held = { w: Math.max(before.width, after.width), h: Math.max(before.height, after.height) };
  });

  // The window takes the size of the content; without content it is hidden.
  $effect(() => {
    // The widget appears again: the window has to be placed again for content that is still there.
    const shown = feed.open;
    if (!kept || !box) {
      placed = false;
      panelFit(null, null).catch(() => {});
      return;
    }
    if (!shown) return;
    const r = held ? { width: held.w, height: held.h } : box.getBoundingClientRect();
    panelFit(r.width, r.height).catch(() => {});
  });

  const LABELS: Record<PanelAction, string> = { copy: "widget.copy", copy_detail: "panel.copy_detail", open_engines: "panel.open_settings", open_history: "panel.open_history" };

  function run(action: PanelAction) {
    if (action === "copy" && kept?.result) widgetCopy(kept.result).catch(() => {});
    else if (action === "copy_detail" && kept?.detail) widgetCopy(kept.detail).catch(() => {});
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

{#if kept}
  <div class="panel" class:open data-edge={edge} bind:this={box} style:width={held ? held.w + "px" : null} style:height={held ? held.h + "px" : null}>
    {#if kept.caption}<div class="caption">{kept.caption}</div>{/if}
    {#if kept.title}
      <div class="head">
        <div class="title">{kept.title}</div>
        {#if kept.close}<button type="button" class="close" onclick={dismiss} aria-label={t("widget.dismiss")} title={t("widget.dismiss")}><Icon name="x" size={14 * feed.scale} /></button>{/if}
      </div>
    {/if}
    {#if kept.text}<div class="text">{kept.text}</div>{/if}
    {#if kept.rewrite}
      <div class="text">{#key pair}<Rewrite from={kept.rewrite.from} to={kept.rewrite.to} animate={!still.matches} ondone={() => { held = null; }} />{/key}</div>
    {/if}
    {#if kept.body}<div class="body">{kept.body}</div>{/if}
    {#if kept.result}<div class="quote result">{kept.result}</div>{/if}
    {#if kept.detail}<div class="quote">{kept.detail}</div>{/if}
    {#if kept.actions.length}
      <div class="actions">
        {#each kept.actions as action (action)}<button type="button" onclick={() => run(action)}>{t(LABELS[action])}</button>{/each}
      </div>
    {/if}
  </div>
  {#if kept.rewrite}
    <div class="panel ghost" bind:this={ghost} aria-hidden="true"><div class="caption">{kept.caption}</div><div class="text">{kept.rewrite.to}</div></div>
  {/if}
{/if}

<style>
  /* Sized by its content, whatever the size of the window at that moment. */
  .panel { position: fixed; left: 0; top: 0; width: max-content; min-width: calc(200 * var(--u)); max-width: calc(372 * var(--u)); border-radius: calc(20 * var(--u)); padding: calc(18 * var(--u)) calc(20 * var(--u)); background: var(--pill-bg); color: var(--pill-ink); box-shadow: inset 0 0 0 1px var(--pill-line); font-size: calc(13 * var(--u)); line-height: 1.6; user-select: none; -webkit-user-select: none; cursor: default; }
  /* Opens from the edge that faces the widget and closes towards it; the content does not move. */
  .panel { --r: calc(20 * var(--u)); opacity: 0; clip-path: inset(100% 0 0 0 round var(--r)); transition: clip-path 130ms cubic-bezier(.4, 0, 1, 1), opacity 40ms linear 130ms; }
  .panel[data-edge="top"] { clip-path: inset(0 0 100% 0 round var(--r)); }
  .panel[data-edge="left"] { clip-path: inset(0 100% 0 0 round var(--r)); }
  .panel[data-edge="right"] { clip-path: inset(0 0 0 100% round var(--r)); }
  .panel.open { opacity: 1; clip-path: inset(0 0 0 0 round var(--r)); transition: opacity 40ms linear, clip-path 200ms cubic-bezier(.2, .8, .2, 1) 40ms; }
  @media (prefers-reduced-motion: reduce) {
    .panel, .panel.open { transition: none; }
  }
  .ghost { visibility: hidden; pointer-events: none; }
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
