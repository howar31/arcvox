<script lang="ts">
  /** One large figure with its name; `parts` pairs each number with its unit. */
  let { parts, label, detail }: { parts: { value: string; unit?: string }[]; label: string; detail?: string } = $props();
</script>

<div class="tile">
  <div class="value num">{#each parts as p, i (i)}<span>{p.value}</span>{#if p.unit}<small>{p.unit}</small>{/if}{/each}</div>
  <div class="label">{label}</div>
  {#if detail}<div class="detail">{detail}</div>{/if}
</div>

<style>
  .tile { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); padding: var(--card-pad); min-width: 0; }
  .value { font: 700 var(--text-stat)/1.1 var(--font-round); white-space: nowrap; }
  small { font: 600 var(--text)/1 var(--font); color: var(--ink-2); margin: 0 6px 0 4px; }
  .label { color: var(--ink-2); font-size: var(--text-sm); margin-top: 10px; }
  .detail { color: var(--ink-3); font-size: var(--text-xs); margin-top: 3px; }
</style>
