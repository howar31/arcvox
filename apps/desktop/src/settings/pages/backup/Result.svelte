<script lang="ts">
  import Icon from "../../../lib/ui/Icon.svelte";
  export interface Line { tone: "ok" | "warn"; text: string; detail?: string; action?: { label: string; run: () => void } }
  let { lines }: { lines: Line[] } = $props();
</script>

<ul class="result">
  {#each lines as line, i (i)}
    <li>
      <span class="mark {line.tone}"><Icon name={line.tone === "ok" ? "check" : "warning"} /></span>
      <span class="text">{line.text}{#if line.detail}<small>{line.detail}</small>{/if}</span>
      {#if line.action}<button type="button" class="btn quiet" onclick={line.action.run}>{line.action.label}</button>{/if}
    </li>
  {/each}
</ul>

<style>
  .result { list-style: none; margin: 0; padding: 0; border: 1px solid var(--line); border-radius: 12px; color: var(--ink); }
  li { display: flex; align-items: flex-start; gap: 10px; padding: 10px 14px; }
  li + li { border-top: 1px solid var(--line); }
  .mark { display: flex; margin-top: 4px; }
  .mark.ok { color: var(--ok); }
  .mark.warn { color: var(--warn); }
  .text { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  small { display: block; color: var(--ink-2); font-size: var(--text-xs); }
  .btn { height: 26px; flex: none; }
</style>
