<script lang="ts">
  /** Level bars; each value is clamped to 0.08..1 so silent bars stay visible. */
  let { levels }: { levels: number[] } = $props();
</script>

<span class="meter" aria-hidden="true">
  {#each levels as l, i (i)}<i style:height="{Math.round(Math.max(0.08, Math.min(1, l)) * 100)}%"></i>{/each}
</span>

<style>
  .meter { display: inline-flex; align-items: center; gap: 2px; height: 20px; flex: none; }
  i { display: block; width: 3px; border-radius: 1.5px; background: var(--live); transition: height 60ms linear; }
</style>
