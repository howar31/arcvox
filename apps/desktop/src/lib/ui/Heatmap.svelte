<script lang="ts">
  /**
   * One square per day, seven to a column, oldest first; `titles` name each day for pointers and
   * screen readers. `months` gives one label per column (the month's name where it begins, "" elsewhere).
   */
  import { tip } from "../tip";
  let { levels, titles, months = [] }: { levels: (0 | 1 | 2 | 3 | 4)[]; titles: string[]; months?: string[] } = $props();
</script>

<div class="heatmap">
  {#if months.some((m) => m)}
    <div class="months" aria-hidden="true">
      {#each months as month, i (i)}<span>{month}</span>{/each}
    </div>
  {/if}
  <div class="heat" role="img" aria-label={titles.filter((_, i) => levels[i] > 0).join("; ")}>
    {#each levels as level, i (i)}<i class="l{level}" use:tip={titles[i]}></i>{/each}
  </div>
</div>

<style>
  /* The newest week is the last column; a wider card adds older weeks before it, up to a year. */
  .heatmap { display: inline-grid; gap: 6px; }
  .months { display: grid; grid-auto-flow: column; grid-auto-columns: 14px; gap: 4px; height: 14px; font-size: 11px; line-height: 14px; color: var(--ink-3); }
  /* A label may run past its 14 px column; the next month is at least four columns away. */
  .months span { white-space: nowrap; overflow: visible; }
  .heat { display: grid; grid-auto-flow: column; grid-template-rows: repeat(7, 14px); grid-auto-columns: 14px; gap: 4px; }
  i { border-radius: 3px; background: var(--sunken); }
  .l1 { background: color-mix(in srgb, var(--accent) 25%, var(--sunken)); }
  .l2 { background: color-mix(in srgb, var(--accent) 50%, var(--sunken)); }
  .l3 { background: color-mix(in srgb, var(--accent) 75%, var(--sunken)); }
  .l4 { background: var(--accent); }
</style>
