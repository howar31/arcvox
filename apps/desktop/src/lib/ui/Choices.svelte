<script lang="ts" generics="V extends string">
  /** One choice out of a few, each on its own line with what it means. */
  import InfoTip from "./InfoTip.svelte";
  let { options, value = $bindable(), label }: {
    /** `info` is the second layer of an option's help, on the card of the ⓘ at the end of its line. */
    options: { value: V; label: string; help?: string; info?: string }[];
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
    <div class="opt" class:on={o.value === value}>
      <div class="line">
        <button
          type="button"
          role="radio"
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
        {#if o.info}<InfoTip info={o.info} />{/if}
      </div>
    </div>
  {/each}
</div>

<style>
  .choices { display: flex; flex-direction: column; gap: 2px; }
  .opt { border-radius: var(--radius); }
  .opt:hover { background: var(--sunken); }
  .opt.on { background: var(--accent-soft); }
  .line { display: flex; align-items: flex-start; }
  .line :global(.info-btn) { flex: none; margin: 10px 10px 0 0; }
  button { display: flex; flex: 1; align-items: flex-start; gap: 12px; min-width: 0; padding: 9px 12px; border: 0; border-radius: var(--radius); background: none; color: var(--ink); text-align: left; cursor: default; }
  .mark { flex: none; display: grid; place-items: center; width: 18px; height: 18px; margin-top: 3px; border-radius: 50%; border: 2px solid var(--line-strong); }
  .on .mark { border-color: var(--accent); }
  .on .mark::after { content: ""; width: 8px; height: 8px; border-radius: 50%; background: var(--accent); }
  .text { display: block; min-width: 0; }
  .name { display: block; }
  small { display: block; margin-top: 1px; color: var(--ink-2); font-size: var(--text-sm); overflow-wrap: anywhere; }
</style>
