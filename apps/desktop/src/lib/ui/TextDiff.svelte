<script lang="ts">
  /** A text with what changed from an earlier one marked: removed units struck through, added ones highlighted. */
  import { diff } from "../textDiff";
  let { from, to }: { from: string | null; to: string } = $props();
  const changes = $derived(from === null ? [{ kind: "keep" as const, text: to }] : diff(from, to));
</script>

<p class="diff">
  {#each changes as c, i (i)}
    {#if c.kind === "del"}<del>{c.text}</del>{:else if c.kind === "ins"}<ins>{c.text}</ins>{:else}{c.text}{/if}
  {/each}
</p>

<style>
  .diff { margin: 0; line-height: 1.75; overflow-wrap: anywhere; white-space: pre-wrap; }
  del { background: color-mix(in srgb, var(--live) 14%, transparent); text-decoration: line-through; text-decoration-color: color-mix(in srgb, var(--live) 60%, transparent); color: var(--ink-2); border-radius: 3px; padding: 0 2px; }
  ins { background: color-mix(in srgb, var(--ok) 16%, transparent); text-decoration: none; border-radius: 3px; padding: 0 2px; }
</style>
