<script lang="ts" generics="V extends string">
  import Bang from "./Bang.svelte";
  let { options, value = $bindable(), label }: {
    /** `problem`, when set, puts the problem mark after the label with this text. */
    options: { value: V; label: string; problem?: string }[];
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
    >{o.label}{#if o.problem}<Bang title={o.problem} />{/if}</button>
  {/each}
</div>

<style>
  .seg { display: inline-flex; padding: 3px; border-radius: calc(var(--radius) + 2px); background: var(--sunken); border: 1px solid var(--line); }
  button { display: inline-flex; align-items: center; gap: 7px; height: calc(var(--control) - 6px); padding: 0 16px; border: 0; border-radius: var(--radius); font-size: var(--text-control); background: none; color: var(--ink-2); white-space: nowrap; cursor: default; }
  button.on { background: var(--surface); color: var(--ink); font-weight: 600; box-shadow: var(--lift); }
</style>
