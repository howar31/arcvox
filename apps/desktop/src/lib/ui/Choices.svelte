<script lang="ts" generics="V extends string">
  /** One choice out of a few, each on its own line with what it means. */
  let { options, value = $bindable(), label }: {
    options: { value: V; label: string; help?: string }[];
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

<div class="choices" role="radiogroup" aria-label={label}>
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
    >
      <span class="mark"></span>
      <span class="text">
        <span class="name">{o.label}</span>
        {#if o.help}<small>{o.help}</small>{/if}
      </span>
    </button>
  {/each}
</div>

<style>
  .choices { display: flex; flex-direction: column; gap: 2px; }
  button { display: flex; align-items: flex-start; gap: 12px; width: 100%; padding: 9px 12px; border: 0; border-radius: var(--radius); background: none; color: var(--ink); text-align: left; cursor: default; }
  button:hover { background: var(--sunken); }
  button.on { background: var(--accent-soft); }
  .mark { flex: none; display: grid; place-items: center; width: 18px; height: 18px; margin-top: 3px; border-radius: 50%; border: 2px solid var(--line-strong); }
  .on .mark { border-color: var(--accent); }
  .on .mark::after { content: ""; width: 8px; height: 8px; border-radius: 50%; background: var(--accent); }
  .text { display: block; min-width: 0; }
  .name { display: block; }
  small { display: block; margin-top: 1px; color: var(--ink-2); font-size: var(--text-sm); overflow-wrap: anywhere; }
</style>
