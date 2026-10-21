<script lang="ts">
  /** The ⓘ beside a name and the card it shows: the second layer of a setting's help. The same card as
      every other tooltip (`lib/tip.ts`), with two additions: it can hold links, so it stays while the
      pointer is on the card too, and a click pins it until a click elsewhere or Esc. */
  import type { Snippet } from "svelte";
  import { onDestroy, tick } from "svelte";
  import { t } from "../i18n";
  import { closed, HIDE_MS, opened, placeCard, showDelay } from "../tip";
  import Icon from "./Icon.svelte";

  let { info }: { info: string | Snippet } = $props();

  let mark = $state<HTMLButtonElement | null>(null);
  let card = $state<HTMLDivElement | null>(null);
  let open = $state(false);
  let pinned = $state(false);
  let showTimer: ReturnType<typeof setTimeout> | undefined;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;

  async function show() {
    clearTimeout(hideTimer);
    if (open) return;
    opened(close);
    open = true;
    await tick();
    place();
  }

  function place() {
    if (!mark || !card) return;
    // The card is positioned against the viewport, so it leaves the page tree: an ancestor with a
    // transform (a sliding fold, a page split) would otherwise become its containing block.
    if (card.parentNode !== document.body) document.body.appendChild(card);
    placeCard(card, mark);
    card.style.visibility = "visible";
  }

  function close() {
    clearTimeout(showTimer);
    clearTimeout(hideTimer);
    if (!open) return;
    open = false;
    pinned = false;
    closed(close);
  }

  function enter() {
    clearTimeout(hideTimer);
    if (open) return;
    clearTimeout(showTimer);
    showTimer = setTimeout(show, showDelay());
  }

  function leave() {
    clearTimeout(showTimer);
    if (pinned) return;
    hideTimer = setTimeout(close, HIDE_MS);
  }

  function toggle() {
    if (pinned) {
      close();
    } else {
      pinned = true;
      show();
    }
  }

  function onDocumentDown(e: PointerEvent) {
    if (!open) return;
    const target = e.target as Node;
    if (mark?.contains(target) || card?.contains(target)) return;
    close();
  }

  function onKey(e: KeyboardEvent) {
    if (open && e.key === "Escape") close();
  }

  // The card was moved under the body, out of the component's own nodes, so the component's removal
  // (a link on the card leading to another page) does not take it along: it goes here.
  onDestroy(() => {
    close();
    card?.remove();
  });
</script>

<svelte:document onpointerdown={onDocumentDown} onkeydown={onKey} onscrollcapture={() => { if (open && !pinned) close(); }} />
<svelte:window onresize={() => { if (open) place(); }} />

<button
  type="button"
  class="info-btn"
  class:open
  bind:this={mark}
  aria-label={t("common.info")}
  aria-expanded={open}
  onpointerenter={enter}
  onpointerleave={leave}
  onclick={toggle}
>
  <Icon name="info" size={14} />
</button>
{#if open}
  <div class="tipcard" role="tooltip" bind:this={card} style="visibility:hidden" onpointerenter={() => clearTimeout(hideTimer)} onpointerleave={leave}>
    {#if typeof info === "string"}{info}{:else}{@render info()}{/if}
  </div>
{/if}

<style>
  .info-btn { display: inline-flex; align-items: center; justify-content: center; width: 20px; height: 20px; margin: -3px 0; padding: 0; border: 0; border-radius: 50%; background: none; color: var(--ink-3); cursor: default; vertical-align: middle; }
  .info-btn:hover, .info-btn.open { color: var(--accent); }
  .info-btn:focus-visible { outline-offset: 1px; }
</style>
