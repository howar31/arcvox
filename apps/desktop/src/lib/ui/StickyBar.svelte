<script lang="ts">
  /** A page's toolbar that stays at the top of the page once scrolled past, without moving anything when it sticks. */
  import { onMount, type Snippet } from "svelte";
  let { children }: { children: Snippet } = $props();
  let mark: HTMLDivElement | undefined = $state();
  let stuck = $state(false);

  onMount(() => {
    const area = mark?.closest<HTMLElement>(".main");
    if (!area || !mark) return;
    const watch = new IntersectionObserver(([e]) => {
      stuck = !e.isIntersecting && e.boundingClientRect.top < (e.rootBounds?.top ?? 0);
    }, { root: area });
    watch.observe(mark);
    return () => watch.disconnect();
  });
</script>

<!-- Where the bar starts: once this has scrolled out of view, the bar is stuck. -->
<div class="mark" bind:this={mark}></div>
<div class="stick" class:stuck>{@render children()}</div>

<style>
  /* The band above the bar belongs to the sticky box at all times (the mark's margin cancels it in the layout),
     so sticking moves nothing; the box keeps its children's margins inside, on the page colour. */
  .mark { height: 0; margin-top: -12px; }
  .stick { position: sticky; top: 0; z-index: 4; display: flow-root; margin: 0 calc(-1 * var(--page-x)); padding: 12px var(--page-x) 0; background: var(--page); }
  .stick.stuck { box-shadow: 0 6px 14px rgba(0, 0, 0, .06); }
</style>
