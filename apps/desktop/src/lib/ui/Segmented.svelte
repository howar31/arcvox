<script lang="ts" generics="V extends string">
  let { options, value = $bindable(), label }: {
    options: { value: V; label: string }[];
    value: V;
    label?: string;
  } = $props();

  let buttons: HTMLButtonElement[] = $state([]);

  function onkeydown(e: KeyboardEvent, i: number) {
    const step = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const next = (i + step + options.length) % options.length;
    value = options[next].value;
    buttons[next]?.focus();
  }
</script>

<div class="seg" role="radiogroup" aria-label={label}>
  {#each options as o, i (o.value)}
    <button
      type="button"
      role="radio"
      class:on={o.value === value}
      aria-checked={o.value === value}
      tabindex={o.value === value ? 0 : -1}
      bind:this={buttons[i]}
      onclick={() => (value = o.value)}
      onkeydown={(e) => onkeydown(e, i)}
    >{o.label}</button>
  {/each}
</div>

<style>
  .seg { display: inline-flex; padding: 3px; border-radius: 10px; background: var(--sunken); border: 1px solid var(--line); }
  button { height: 28px; padding: 0 16px; border: 0; border-radius: 7px; font-size: var(--text-control); background: none; color: var(--ink-2); white-space: nowrap; cursor: default; }
  button.on { background: var(--surface); color: var(--ink); font-weight: 600; box-shadow: var(--lift); }
</style>
