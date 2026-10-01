<script lang="ts">
  /** Named values as horizontal bars, each against the largest. */
  let { rows }: { rows: { label: string; value: number; note: string; muted?: boolean }[] } = $props();
  const max = $derived(Math.max(1, ...rows.map((r) => r.value)));
</script>

<div class="bars">
  {#each rows as r, i (i)}
    <div class="bar">
      <span class="label" class:muted={r.muted} title={r.label}>{r.label}</span>
      <span class="track"><i style:width="{Math.max(r.value > 0 ? 2 : 0, (r.value / max) * 100)}%"></i></span>
      <span class="note num">{r.note}</span>
    </div>
  {/each}
</div>

<style>
  .bars { display: flex; flex-direction: column; gap: 12px; }
  .bar { display: grid; grid-template-columns: minmax(0, 7.5em) minmax(0, 1fr) auto; align-items: center; gap: 12px; font-size: var(--text-control); }
  .label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .label.muted { color: var(--ink-2); }
  .track { height: 10px; border-radius: 5px; background: var(--sunken); overflow: hidden; }
  .track i { display: block; height: 100%; border-radius: 5px; background: var(--accent); }
  .note { text-align: right; color: var(--ink-2); font-size: var(--text-sm); white-space: nowrap; }
</style>
