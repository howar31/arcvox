<script lang="ts">
  export interface CheckItem { key: string; label: string; help?: string; warn?: string; disabled?: boolean }
  let { items, value = $bindable({}) }: { items: CheckItem[]; value?: Record<string, boolean> } = $props();
</script>

<div class="checks">
  {#each items as item (item.key)}
    <label class="check" class:off={item.disabled}>
      <input type="checkbox" checked={!item.disabled && !!value[item.key]} disabled={item.disabled} onchange={(e) => { value = { ...value, [item.key]: e.currentTarget.checked }; }} />
      <span class="text">
        <span class="name">{item.label}</span>
        {#if item.help}<small>{item.help}</small>{/if}
        {#if item.warn}<small class="warn">{item.warn}</small>{/if}
      </span>
    </label>
  {/each}
</div>

<style>
  .checks { border: 1px solid var(--line); border-radius: 12px; color: var(--ink); }
  .check { display: flex; align-items: flex-start; gap: 12px; padding: 9px 14px; }
  .check + .check { border-top: 1px solid var(--line); }
  .check.off { opacity: .5; }
  .check input { margin-top: 2px; }
  .text { min-width: 0; }
  .name { display: block; }
  small { display: block; color: var(--ink-2); font-size: var(--text-xs); overflow-wrap: anywhere; }
  small.warn { color: var(--warn); }
</style>
