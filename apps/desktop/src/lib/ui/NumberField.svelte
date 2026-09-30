<script lang="ts">
  let { value = $bindable(), min, max, unit, label, integer = true }: {
    value: number;
    min?: number;
    max?: number;
    unit?: string;
    /** Accessible name for the input. */
    label?: string;
    integer?: boolean;
  } = $props();

  const invalid = $derived(
    typeof value !== "number" || !Number.isFinite(value) || (integer && !Number.isInteger(value))
      || (min !== undefined && value < min) || (max !== undefined && value > max),
  );
</script>

<span class="field">
  <input type="number" bind:value {min} {max} step={integer ? 1 : "any"} aria-label={label} aria-invalid={invalid} />
  {#if unit}<span class="unit">{unit}</span>{/if}
</span>

<style>
  .field { display: inline-flex; align-items: center; gap: 8px; flex: none; }
  .unit { color: var(--ink-2); font-size: 12px; min-width: 24px; white-space: nowrap; }
</style>
