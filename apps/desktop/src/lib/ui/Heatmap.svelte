<script lang="ts">
  /** One square per day, seven to a column, oldest first; `titles` name each day for pointers and screen readers. */
  let { levels, titles }: { levels: (0 | 1 | 2 | 3 | 4)[]; titles: string[] } = $props();
</script>

<div class="heat" role="img" aria-label={titles.filter((_, i) => levels[i] > 0).join("; ")}>
  {#each levels as level, i (i)}<i class="l{level}" title={titles[i]}></i>{/each}
</div>

<style>
  .heat { display: grid; grid-auto-flow: column; grid-template-rows: repeat(7, 14px); grid-auto-columns: 14px; gap: 4px; }
  i { border-radius: 3px; background: var(--sunken); }
  .l1 { background: color-mix(in srgb, var(--accent) 25%, var(--sunken)); }
  .l2 { background: color-mix(in srgb, var(--accent) 50%, var(--sunken)); }
  .l3 { background: color-mix(in srgb, var(--accent) 75%, var(--sunken)); }
  .l4 { background: var(--accent); }
</style>
