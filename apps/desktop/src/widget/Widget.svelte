<script lang="ts">
  import { onMount } from "svelte";
  import { widgetDragStart, widgetMeasure, widgetMenu } from "../lib/api";
  import { t } from "../lib/i18n";
  import { LevelHistory, TICK, eased, mapped } from "../lib/level";
  import { LABEL_KEYS, READY, RING_AT_REST, labelKey, ringView } from "../lib/widgetView";
  import { WidgetFeed } from "../lib/widgetEvents.svelte";

  const feed = new WidgetFeed();
  // The ring, the gap and the padding around an empty label, and a label written across and downwards.
  let rest: HTMLElement, across: HTMLElement, down: HTMLElement;
  // The level while listening: two outlines around the ring, and bars passing behind the label.
  let inner = $state<HTMLElement | null>(null), outer = $state<HTMLElement | null>(null), canvas: HTMLCanvasElement;
  // In units. A sample of the level moves `step` along the widget; nothing is drawn around the ring, the bars fade in from `from` to `to`.
  const WAVE = { alpha: 0.2, step: 2, pitch: 5, thick: 3, from: 40, to: 58 };

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

  $effect(() => {
    if (!listening || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const history = new LevelHistory(160);
    const color = getComputedStyle(canvas).color;
    let level = 0, sampled = performance.now(), painted = 0;
    const paint = () => {
      const now = painted = performance.now();
      // The picture moves on between two samples: the newest one is reached when the next is due.
      const age = 1 - Math.min(1, (now - sampled) / TICK);
      if (inner) outline(inner, history.at(age), 0.7, 0.68);
      if (outer) outline(outer, history.at(age + 2), 1.4, 0.54);
      wave(history, age, now, color);
    };
    const timer = setInterval(() => {
      // Frames are not delivered to a window the system counts as hidden: the level is still shown, at this pace.
      if (sampled - painted > TICK) paint();
      level = eased(level, mapped(feed.takeLevel()));
      history.push(level);
      sampled = performance.now();
    }, TICK);
    let frame = requestAnimationFrame(function next() {
      paint();
      frame = requestAnimationFrame(next);
    });
    return () => {
      clearInterval(timer);
      cancelAnimationFrame(frame);
      canvas.getContext("2d")!.clearRect(0, 0, canvas.width, canvas.height);
    };
  });

  /** An outline whose radius is the level: on the ring at silence, `gain` ring radii further at full level. */
  function outline(node: HTMLElement, level: number, gain: number, strength: number) {
    node.style.transform = `scale(${(1 + gain * level).toFixed(3)})`;
    node.style.opacity = (0.12 + strength * level).toFixed(2);
  }

  /** The last seconds as bars, the newest at the widget's end, moving towards the ring. */
  function wave(history: LevelHistory, age: number, now: number, color: string) {
    const u = feed.scale, dpr = window.devicePixelRatio || 1;
    const w = canvas.clientWidth, h = canvas.clientHeight;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
    }
    const g = canvas.getContext("2d")!;
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.clearRect(0, 0, canvas.width, canvas.height);
    // Drawn along the long side (x) and across the short side (y); an upright widget swaps the two.
    const upright = h > w;
    if (upright) g.setTransform(0, dpr, dpr, 0, 0, 0);
    else g.setTransform(dpr, 0, 0, dpr, 0, 0);
    const along = upright ? h : w, mid = (upright ? w : h) / 2;
    const step = WAVE.step * u, pitch = WAVE.pitch * u, thick = WAVE.thick * u, reach = mid - 3 * u;
    g.globalAlpha = WAVE.alpha;
    g.fillStyle = color;
    for (let x = along - (((now / TICK) * step) % pitch); x > WAVE.from * u - pitch; x -= pitch) {
      const half = thick / 2 + reach * history.at((along - x - thick / 2) / step + age);
      g.beginPath();
      g.roundRect(x, mid - half, thick, half * 2, thick / 2);
      g.fill();
    }
    const fade = g.createLinearGradient(WAVE.from * u, 0, WAVE.to * u, 0);
    fade.addColorStop(0, "rgba(0,0,0,0)");
    fade.addColorStop(1, "rgba(0,0,0,1)");
    g.globalAlpha = 1;
    g.globalCompositeOperation = "destination-in";
    g.fillStyle = fade;
    g.fillRect(0, 0, along, mid * 2);
    g.globalCompositeOperation = "source-over";
  }

  // With no dictation going on the widget is off the screen, or rests on it: folded to its round end, or whole with a label.
  const resting = $derived(!feed.open && feed.idle !== "hidden");
  const ring = $derived(resting ? RING_AT_REST : ringView(feed.state, feed.message, feed.inserted));
  const label = $derived.by(() => {
    void feed.language;
    const key = resting ? (feed.idle === "label" ? READY : null) : labelKey(feed.state, feed.message, feed.inserted);
    return key ? t(key) : "";
  });

  // The widget can be dragged; the app moves the window.
  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0) return;
    widgetDragStart().catch(() => {});
  }

  // The right button opens the menu of the menu-bar icon.
  function oncontextmenu(e: MouseEvent) {
    e.preventDefault();
    widgetMenu().catch(() => {});
  }
</script>

<svelte:window {oncontextmenu} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="pill" class:listening class:open={feed.open || (resting && feed.idle === "label")} class:rest={resting && feed.idle === "ring"} {onpointerdown}>
  <canvas class="wave" bind:this={canvas} aria-hidden="true"></canvas>
  <!-- A new ring when its colour or its motion changes, so the motion plays from its start. -->
  {#key ring.tone + ring.motion}
    <span class="ring {ring.tone} {ring.motion}" aria-hidden="true">
      {#if ring.motion === "level"}<span class="track" bind:this={inner}></span><span class="track" bind:this={outer}></span>{/if}
      {#if ring.motion === "once" || ring.motion === "twice"}<span class="pulse"></span>{/if}
      {#if ring.glyph}
        <svg viewBox="0 0 14 14">
          {#if ring.glyph === "check"}<path pathLength="1" d="M3.6 7.4 L6.1 9.8 L10.5 4.7" />
          {:else if ring.glyph === "bang"}<path pathLength="1" d="M7 3.3 V7.9" /><circle cx="7" cy="10.5" r="1" />
          {:else}<path pathLength="1" d="M4.5 4.5 L9.5 9.5 M9.5 4.5 L4.5 9.5" />{/if}
        </svg>
      {/if}
    </span>
  {/key}
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
  .pill { position: relative; width: 100vw; height: 100vh; border-radius: calc(24 * var(--u)); overflow: hidden; background: var(--pill-bg); color: var(--pill-ink); box-shadow: inset 0 0 0 1px var(--pill-line); user-select: none; -webkit-user-select: none; cursor: default; }
  /* Appearing: the round end with the ring first, then the widget opens towards its label, which comes last. Leaving is the same backwards, and shorter. */
  .pill { opacity: 0; clip-path: inset(0 calc(100% - 48 * var(--u)) 0 0 round calc(24 * var(--u))); transition: clip-path 140ms cubic-bezier(.4, 0, 1, 1), opacity 60ms linear 140ms; }
  .pill.open { opacity: 1; clip-path: inset(0 0 0 0 round calc(24 * var(--u))); transition: opacity 80ms linear, clip-path 200ms cubic-bezier(.2, .8, .2, 1) 80ms; }
  /* At rest only the round end shows; it stays when the widget closes and is where it opens from. */
  .pill.rest { opacity: 1; transition: clip-path 140ms cubic-bezier(.4, 0, 1, 1); }
  .label { opacity: 0; transition: opacity 60ms linear; }
  .pill.open .label { opacity: 1; transition: opacity 120ms linear 160ms; }
  .measure, .probe { position: fixed; left: 0; top: 0; width: max-content; visibility: hidden; pointer-events: none; }
  /* Behind the ring and the label. */
  .wave { position: absolute; left: 0; top: 0; width: 100%; height: 100%; color: var(--pill-accent); pointer-events: none; }
  .ring, .label { position: relative; }
  /* The colour is the kind of outcome; red is for a failure only. */
  .ring { --c: var(--pill-ink-2); flex: none; width: calc(20 * var(--u)); height: calc(20 * var(--u)); border-radius: 50%; border: calc(3 * var(--u)) solid var(--c); color: var(--c); }
  .ring.accent { --c: var(--pill-accent); }
  .ring.ok { --c: var(--pill-ok); }
  .ring.warn { --c: var(--pill-warn); }
  .ring.fail { --c: var(--pill-fail); }
  .track, .pulse { position: absolute; inset: calc(-3 * var(--u)); border-radius: 50%; border: calc(1.5 * var(--u)) solid var(--c); opacity: 0; pointer-events: none; }
  svg { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; }
  path { fill: none; stroke: currentColor; stroke-width: 1.9; stroke-linecap: round; stroke-linejoin: round; stroke-dasharray: 1; stroke-dashoffset: 1; animation: draw 260ms ease-out 120ms forwards; }
  circle { fill: currentColor; opacity: 0; animation: appear 120ms linear 300ms forwards; }
  /* What is going on: a gap going round while it works, closing when the text goes in; a dashed ring can be dragged. */
  .ring.spin { border-right-color: transparent; animation: spin .9s linear infinite; }
  .ring.close { animation: close 260ms ease-out; }
  .ring.drag { border-style: dashed; animation: spin 7s linear infinite; }
  /* How it ended: an outline leaves the ring once when the text went in, twice when it needs attention; the ring settles when nothing came of it and shakes on a failure. */
  .ring.once .pulse { animation: pulse 640ms ease-out 80ms 1; }
  .ring.twice .pulse { animation: pulse 640ms ease-out 80ms 2; }
  .ring.settle { animation: settle 320ms ease-out; }
  .ring.shake { animation: shake 380ms ease-in-out; }
  .probe.down { writing-mode: vertical-rl; letter-spacing: .12em; }
  /* Vertical: the ring on top, the label written downwards. */
  @media (orientation: portrait) {
    .pill { flex-direction: column; padding: calc(14 * var(--u)) 0 calc(22 * var(--u)); }
    .pill:not(.open) { clip-path: inset(0 0 calc(100% - 48 * var(--u)) 0 round calc(24 * var(--u))); }
    .ring.shake { animation-name: shake-down; }
    .label { writing-mode: vertical-rl; letter-spacing: .12em; }
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  @keyframes close { 0% { transform: scale(.82); } 60% { transform: scale(1.1); } 100% { transform: scale(1); } }
  @keyframes pulse { from { transform: scale(1); opacity: .8; } to { transform: scale(2.2); opacity: 0; } }
  @keyframes settle { from { transform: scale(.72); opacity: .3; } to { transform: scale(1); opacity: 1; } }
  @keyframes shake { 0%, 100% { transform: translateX(0); } 20% { transform: translateX(calc(-3.5 * var(--u))); } 40% { transform: translateX(calc(3.5 * var(--u))); } 60% { transform: translateX(calc(-2.2 * var(--u))); } 80% { transform: translateX(calc(1.4 * var(--u))); } }
  @keyframes shake-down { 0%, 100% { transform: translateY(0); } 20% { transform: translateY(calc(-3.5 * var(--u))); } 40% { transform: translateY(calc(3.5 * var(--u))); } 60% { transform: translateY(calc(-2.2 * var(--u))); } 80% { transform: translateY(calc(1.4 * var(--u))); } }
  @keyframes draw { to { stroke-dashoffset: 0; } }
  @keyframes appear { to { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) {
    /* Nothing moves: the ring is whole, its mark is drawn, and one still outline says that it listens. */
    .ring.spin, .ring.close, .ring.drag, .ring.settle, .ring.shake, .ring.once .pulse, .ring.twice .pulse, path, circle { animation: none; }
    .ring.spin { border-right-color: var(--c); }
    path { stroke-dashoffset: 0; }
    circle { opacity: 1; }
    .listening .track:first-child { opacity: .5; transform: scale(1.5); }
    .pill, .pill.open, .label, .pill.open .label { transition: none; }
  }
</style>
