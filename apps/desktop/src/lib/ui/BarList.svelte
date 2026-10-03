<script lang="ts">
  /**
   * Named values as horizontal bars, each against the largest; `columns` lays the rows out side by
   * side. The note column has one width, so every bar starts and ends on the same lines; when the
   * bars would be too short to compare, only the names and notes are shown.
   */
  let { rows, columns = 1 }: { rows: { label: string; value: number; note: string; muted?: boolean }[]; columns?: number } = $props();
  const max = $derived(Math.max(1, ...rows.map((r) => r.value)));
  let width = $state(0);
  /** Label 7.5em, note 6.5em and two gaps: what is left is the bar. */
  const MIN_TRACK = 56;
  const bars = $derived(width === 0 || width / columns - 14 * 14 - 24 >= MIN_TRACK);
</script>

<div class="bars" class:nobars={!bars} style:grid-template-columns="repeat({columns}, minmax(0, 1fr))" bind:clientWidth={width}>
  {#each rows as r, i (i)}
    <div class="bar">
      <span class="label" class:muted={r.muted} title={r.label}>{r.label}</span>
      {#if bars}<span class="track"><i style:width="{Math.max(r.value > 0 ? 2 : 0, (r.value / max) * 100)}%"></i></span>{/if}
      <span class="note num">{r.note}</span>
    </div>
  {/each}
</div>

<style>
  .bars { display: grid; gap: 12px 28px; align-content: start; }
  .bar { display: grid; grid-template-columns: minmax(0, 7.5em) minmax(0, 1fr) 6.5em; align-items: center; gap: 12px; font-size: var(--text-control); }
  .nobars .bar { grid-template-columns: minmax(0, 1fr) auto; }
  .label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .label.muted { color: var(--ink-2); }
  .track { height: 10px; border-radius: 5px; background: var(--sunken); overflow: hidden; }
  .track i { display: block; height: 100%; border-radius: 5px; background: var(--accent); }
  .note { text-align: right; color: var(--ink-2); font-size: var(--text-sm); white-space: nowrap; }
</style>
