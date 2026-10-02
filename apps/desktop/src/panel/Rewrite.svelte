<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { diff } from "../lib/textDiff";
  import { LEAD, MOVE, REST } from "../lib/rewriteTiming";

  // The recognised text turns into the final text: what goes is struck through first, what comes arrives highlighted.
  let { from, to, animate, ondone }: { from: string; to: string; animate: boolean; ondone: () => void } = $props();

  // One instance plays one pair of texts once; another pair gets another instance.
  const moving = untrack(() => animate && from !== to);
  const changes = untrack(() => (moving ? diff(from, to) : []));
  // "laid": every unit at its own width, to measure it, before anything is shown.
  let phase = $state<"laid" | "struck" | "moved" | "settled" | "done">(moving ? "laid" : "done");
  /** Changes of phase are animated only once the first phase is in place. */
  let armed = $state(false);
  let line: HTMLElement | undefined = $state();

  onMount(() => {
    if (!moving) {
      ondone();
      return;
    }
    // A unit closes from, or opens to, its own width.
    for (const el of line!.children) (el as HTMLElement).style.maxWidth = Math.ceil(el.getBoundingClientRect().width) + 1 + "px";
    phase = "struck";
    tick().then(() => {
      line?.getBoundingClientRect();
      armed = true;
    });
    const timers = [
      setTimeout(() => { phase = "moved"; }, LEAD),
      setTimeout(() => { phase = "settled"; }, LEAD + MOVE + REST),
      setTimeout(() => { phase = "done"; ondone(); }, LEAD + MOVE + REST + MOVE),
    ];
    return () => timers.forEach(clearTimeout);
  });
</script>

{#if phase === "done"}
  {to}
{:else}
  <span class="line {phase}" class:armed bind:this={line}>{#each changes as c, i (i)}<span class="unit {c.kind}">{c.text}</span>{/each}</span>
{/if}

<style>
  /* The transitions last as long as MOVE in rewriteTiming.ts. */
  .unit { display: inline-block; white-space: pre; vertical-align: top; overflow: hidden; }
  .armed .unit { transition: max-width 600ms cubic-bezier(.3, .7, .2, 1), opacity 600ms, color 600ms; }
  .del { text-decoration: line-through; color: var(--pill-fail); }
  .ins { color: var(--pill-accent); }
  .laid { visibility: hidden; }
  /* Before the move the added text takes no room; after it the removed text takes none. */
  .struck .ins, .moved .del, .settled .del { opacity: 0; max-width: 0 !important; }
  .settled .ins { color: inherit; }
</style>
