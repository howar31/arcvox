<script lang="ts" module>
  import type { CellId } from "../../../lib/flowLast";

  /** What the route can select: a stage, or the stop for the modules not enabled. */
  export type Pick = CellId | "enable";

  export interface Cell {
    id: CellId;
    name: string;
    /** The current setting, one line. */
    setting: string;
    /** What the stage did in the last run; "" before the first dictation. */
    line: string;
    /** How long the stage took in the last run; null for a stage without a stored time. */
    time: string | null;
    /** The setting names something missing (a model not downloaded, an engine gone). */
    warn: boolean;
    unknown: boolean;
    /** Set when the stage runs on another engine than the one the profile names: the problem mark, with this text. */
    problem?: string;
  }
</script>

<script lang="ts">
  /** The stages as stops on one line, from sound to text, with the selected stage summed up under it.
   * A last stop stands for the modules the profile has not enabled; it is gone when every module is enabled.
   * The summary's last-run line opens the stage's text from the last run in the same card, under the route.
   * Scrolled past, the route stays at the top of the page; the summary is cut off there so the stops take little room. */
  import { onMount, tick, type Snippet } from "svelte";
  import Bang from "../../../lib/ui/Bang.svelte";
  import Fold from "../../../lib/ui/Fold.svelte";
  import Icon from "../../../lib/ui/Icon.svelte";
  import { t } from "../../../lib/i18n";

  let { cells, selected, onpick, label, entry = "", meta = "", detail, stuckHeight = $bindable(0) }: {
    cells: Cell[]; selected: Pick; onpick: (id: Pick) => void; label: string;
    /** The name of the stop for the modules not enabled; "" when every module is enabled and the stop is not shown. */
    entry?: string;
    /** The selected stage's setting with what it uses, for the summary. */
    meta?: string;
    /** The selected stage's text from the last run; absent when the stage has none to show. */
    detail?: Snippet;
    /** The height the route takes at the top of the page while stuck. */
    stuckHeight?: number;
  } = $props();

  let mark = $state<HTMLElement>();
  let summaryHeight = $state(0);
  let boxHeight = $state(0);
  let stuck = $state(false);
  /** The summary's height from before the route stuck: out of the flow it may lay out differently. */
  let cut = $state(0);
  $effect(() => { if (!stuck) cut = summaryHeight; });
  // Unstuck the box holds the summary; stuck it holds 10 of page colour under the route instead.
  $effect(() => { stuckHeight = stuck ? boxHeight : boxHeight - cut + 10; });
  const current = $derived(cells.find((c) => c.id === selected));
  // The last run's text is folded until asked for; the choice holds for every stage while the page is open.
  let opened = $state(false);
  const open = $derived(detail !== undefined && opened);
  // The card stays joined to its foot until the text has slid shut.
  let closing = $state(false);
  let wasOpen = false;
  $effect.pre(() => { if (wasOpen && !open) closing = true; wasOpen = open; });
  const joined = $derived(open || closing);

  /** The page scrolls in the window's main area; the route sticks to its top. */
  const scroller = () => mark?.closest<HTMLElement>(".main") ?? null;

  async function pick(id: Pick) {
    onpick(id);
    if (!stuck) return;
    // A stage picked while the route is stuck shows its settings from their top, right under the route, which
    // stays stuck: past the place where it sticks by the room the hidden summary keeps.
    await tick();
    const area = scroller();
    if (area && mark) area.scrollTop += mark.getBoundingClientRect().top - area.getBoundingClientRect().top + Math.max(cut - 10, 1);
  }

  onMount(() => {
    const area = scroller();
    if (!area || !mark) return;
    const watch = new IntersectionObserver(([e]) => {
      stuck = !e.isIntersecting && e.boundingClientRect.top < (e.rootBounds?.top ?? 0);
    }, { root: area });
    watch.observe(mark);
    return () => watch.disconnect();
  });
</script>

<!-- Where the route starts: once this has scrolled out of view, the route is stuck. -->
<div class="mark" bind:this={mark}></div>
<div class="stick" class:stuck style:--cut="{cut}px" bind:offsetHeight={boxHeight}>
  <div class="route" class:joined>
    <div class="stops" role="tablist" aria-label={label} style:--n={cells.length + (entry ? 1 : 0)}>
      {#each cells as c, i (c.id)}
        <button type="button" class="stop" class:on={selected === c.id} role="tab" aria-selected={selected === c.id} onclick={() => pick(c.id)}>
          <span class="dot">{i + 1}{#if c.problem}<span class="bang"><Bang tone="warn" title={c.problem} /></span>{/if}</span>
          <span class="name">{c.name}</span>
        </button>
      {/each}
      {#if entry}
        <button type="button" class="stop add" class:on={selected === "enable"} role="tab" aria-selected={selected === "enable"} onclick={() => pick("enable")}>
          <span class="dot"><Icon name="plus" size={13} /></span>
          <span class="name">{entry}</span>
        </button>
      {/if}
    </div>
    <div class="summary" bind:offsetHeight={summaryHeight} aria-hidden={stuck}>
      {#if current}
        <span class="what"><b>{current.name}</b><span class:warn={current.warn} class:unknown={current.unknown}>{meta || current.setting}</span></span>
        {#if detail}
          <button type="button" class="last toggle" aria-expanded={open} onclick={() => (opened = !open)}>
            <span>{current.line}{#if current.time}<em>{t("flow.panel.sep")}{current.time}</em>{/if}</span>
            <span class="chev" class:open><Icon name="chevron-down" size={14} /></span>
          </button>
        {:else}
          <span class="last">{current.line}{#if current.time}<em>{t("flow.panel.sep")}{current.time}</em>{/if}</span>
        {/if}
      {:else}
        <span class="what"><b>{entry}</b></span>
      {/if}
    </div>
  </div>
</div>
<!-- The card's lower edge, outside the sticky box: the stage's text opens here and scrolls away under the stuck route. -->
{#if joined}<div class="foot">{#if open && detail}<Fold onoutroend={() => (closing = false)}>{@render detail()}</Fold>{/if}</div>{/if}

<style>
  /* Sits where the sticky box starts, 16 above the route, so it leaves the view the moment the box sticks. */
  .mark { height: 0; margin-top: -16px; }
  /* The band above the route is part of the sticky box at all times (the mark's margin cancels it in the layout),
     so sticking moves nothing. While stuck the summary leaves the flow and a bottom margin of the same height
     takes its place, so the page below keeps its position; the route shows whole, on the page colour. */
  .stick { position: sticky; top: 0; z-index: 4; margin: 0 calc(-1 * var(--page-x)); padding: 16px var(--page-x) 0; pointer-events: none; }
  .stick.stuck { background: var(--page); padding-bottom: 10px; margin-bottom: calc(var(--cut) - 10px); box-shadow: 0 6px 14px rgba(0, 0, 0, .06); }
  /* Out of the flow but still laid out, so its height stays measured. */
  .stick.stuck .summary { position: absolute; visibility: hidden; }
  .route { pointer-events: auto; background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); padding: 16px 12px 0; }
  /* With the text open the card goes on in the foot below (1 up, under the route's transparent lower border), so
     its lower border is the foot's; the summary draws the line between the two. Stuck the route closes on its own. */
  .stick:not(.stuck) .route.joined { border-bottom-color: transparent; border-radius: var(--radius-group) var(--radius-group) 0 0; }
  .route.joined .summary { box-shadow: 0 1px 0 var(--line); }
  .foot { margin-top: -1px; background: var(--surface); border: 1px solid var(--line); border-top: 0; border-radius: 0 0 var(--radius-group) var(--radius-group); }
  .stops { position: relative; display: grid; grid-template-columns: repeat(var(--n), minmax(0, 1fr)); }
  /* The line through the stops, from the centre of the first to the centre of the last. */
  .stops::before { content: ""; position: absolute; top: 13px; left: calc(50% / var(--n)); right: calc(50% / var(--n)); height: 2px; background: var(--line-strong); }
  .stop { position: relative; display: flex; flex-direction: column; align-items: center; gap: 6px; min-width: 0; padding: 0 2px 12px; border: 0; background: none; color: var(--ink-2); font-size: var(--text-xs); line-height: 1.3; text-align: center; cursor: default; }
  .stop:focus-visible { outline-offset: 2px; border-radius: 8px; }
  .dot { position: relative; display: grid; place-items: center; width: 28px; height: 28px; border-radius: 50%; border: 2px solid var(--surface); background: var(--sunken); color: var(--ink-2); font: 600 var(--text-xs)/1 var(--font); }
  .stop:hover .dot { background: color-mix(in srgb, var(--accent) 12%, var(--sunken)); }
  .stop.on { color: var(--ink); font-weight: 600; }
  .stop.on .dot { background: var(--accent); color: var(--accent-ink); box-shadow: 0 0 0 4px var(--accent-soft); }
  .stop.add .dot { background: var(--surface); border: 1.5px dashed var(--line-strong); color: var(--ink-3); }
  .stop.add.on .dot { border-color: var(--accent); color: var(--accent); box-shadow: none; }
  .bang { position: absolute; top: -6px; right: -9px; }
  .name { max-width: 100%; overflow-wrap: anywhere; }
  .summary { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; margin: 0 4px; padding: 10px 6px 12px; border-top: 1px solid var(--line); font-size: var(--text-sm); }
  .what { display: flex; flex-direction: column; min-width: 0; }
  .what b { font-size: var(--text); }
  .what span { color: var(--ink-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .what span.warn { color: var(--warn); }
  .what span.unknown { color: var(--live); }
  .last { flex: none; max-width: 50%; color: var(--ink-2); text-align: right; }
  .last em { font-style: normal; color: var(--ink-3); }
  .toggle { display: flex; align-items: center; gap: 10px; margin: -4px -6px; padding: 4px 6px; border: 0; border-radius: 8px; background: none; font: inherit; cursor: default; }
  .toggle:hover { background: var(--sunken); }
  .chev { flex: none; display: inline-flex; color: var(--ink-3); transition: transform .15s; }
  .chev.open { transform: rotate(180deg); }
</style>
