<script lang="ts">
  /** Values as vertical columns against the largest, with a few labels spread underneath. */
  let { values, height = 96, labels = [], titles = [] }: { values: number[]; height?: number; labels?: string[]; titles?: string[] } = $props();
  const max = $derived(Math.max(1, ...values));
</script>

<div class="cols" style:height="{height}px">
  {#each values as v, i (i)}<i style:height="{v > 0 ? Math.max(3, (v / max) * 100) : 0}%" title={titles[i]}></i>{/each}
</div>
{#if labels.length}<div class="axis">{#each labels as l, i (i)}<span>{l}</span>{/each}</div>{/if}

<style>
  .cols { display: flex; align-items: flex-end; gap: 3px; border-bottom: 1px solid var(--line); }
  i { flex: 1; min-width: 0; background: var(--accent); border-radius: 3px 3px 0 0; opacity: .85; }
  .axis { display: flex; justify-content: space-between; color: var(--ink-3); font-size: var(--text-xs); margin-top: 6px; }
</style>
