<script lang="ts">
  import { onMount } from "svelte";
  import { widgetDragStart, widgetMeasure } from "../lib/api";
  import { t } from "../lib/i18n";
  import { LABEL_KEYS, labelKey } from "../lib/widgetView";
  import { WidgetFeed } from "../lib/widgetEvents.svelte";

  const feed = new WidgetFeed();
  // The ring, the gap and the padding around an empty label, and a label written across and downwards.
  let rest: HTMLElement, across: HTMLElement, down: HTMLElement;

  onMount(() => feed.listen());

  // The widget has one size for a language: its long side holds the longest label in each shape.
  $effect(() => {
    void feed.language;
    void feed.scale;
    let wide = 0, tall = 0;
    for (const key of LABEL_KEYS) {
      across.textContent = down.textContent = t(key);
      wide = Math.max(wide, across.getBoundingClientRect().width);
      tall = Math.max(tall, down.getBoundingClientRect().height);
    }
    across.textContent = down.textContent = "";
    const chrome = rest.getBoundingClientRect().width;
    widgetMeasure(chrome + wide, chrome + tall).catch(() => {});
  });

  const listening = $derived(feed.state === "recording" || feed.state === "locked");
  // Halo around the live ring, in px: grows with the input level, clamped.
  const halo = $derived(listening ? (2 + 7 * Math.min(1, Math.max(0, feed.level * 4))) * feed.scale : 0);
  const tone = $derived.by(() => {
    if (feed.message) return feed.message.kind === "error" ? "live" : "accent";
    if (listening) return "live";
    if (feed.state === "processing") return "think";
    if (feed.state === "inserting" || feed.state === "placing") return "accent";
    return "off";
  });
  const label = $derived.by(() => {
    void feed.language;
    const key = labelKey(feed.state, feed.message);
    return key ? t(key) : "";
  });

  // The widget can be dragged; the app moves the window.
  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0) return;
    widgetDragStart().catch(() => {});
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="pill" {onpointerdown}>
  <span class="ring {tone}" style:--halo="{halo}px" aria-hidden="true"></span>
  <!-- A new element for each label: a reused one can keep the previous label's length when written downwards. -->
  {#key label}<span class="label">{label}</span>{/key}
</div>
<!-- For measuring. The labels are measured on their own: inside the row, the length of one written downwards is not reliable. -->
<div class="measure" aria-hidden="true" bind:this={rest}><span class="ring"></span><span></span></div>
<span class="probe" aria-hidden="true" bind:this={across}></span>
<span class="probe down" aria-hidden="true" bind:this={down}></span>

<style>
  /* The ring keeps its place whatever the label: it sits in the round end, the same distance from three sides. */
  .pill, .measure { display: flex; align-items: center; gap: calc(14 * var(--u)); padding: 0 calc(22 * var(--u)) 0 calc(14 * var(--u)); }
  .pill, .measure, .probe { font-size: calc(13 * var(--u)); line-height: 1.35; white-space: nowrap; }
  .pill { width: 100vw; height: 100vh; border-radius: calc(24 * var(--u)); overflow: hidden; background: var(--pill-bg); color: var(--pill-ink); box-shadow: inset 0 0 0 1px var(--pill-line); user-select: none; -webkit-user-select: none; cursor: default; }
  .measure, .probe { position: fixed; left: 0; top: 0; width: max-content; visibility: hidden; pointer-events: none; }
  .ring { flex: none; width: calc(20 * var(--u)); height: calc(20 * var(--u)); border-radius: 50%; border: calc(3 * var(--u)) solid var(--pill-ink-2); }
  .ring.live { border-color: var(--pill-live); box-shadow: 0 0 0 var(--halo) var(--pill-live-halo); transition: box-shadow 80ms linear; }
  .ring.accent { border-color: var(--pill-accent); }
  .ring.think { border-color: var(--pill-accent); border-right-color: transparent; animation: spin .9s linear infinite; }
  .probe.down { writing-mode: vertical-rl; letter-spacing: .12em; }
  /* Vertical: the ring on top, the label written downwards. */
  @media (orientation: portrait) {
    .pill { flex-direction: column; padding: calc(14 * var(--u)) 0 calc(22 * var(--u)); }
    .label { writing-mode: vertical-rl; letter-spacing: .12em; }
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) {
    /* Without the spin, a full ring: a still gap would read as a fault. */
    .ring.think { animation: none; border-right-color: var(--pill-accent); }
    .ring.live { box-shadow: 0 0 0 calc(5 * var(--u)) var(--pill-live-halo); transition: none; }
  }
</style>
