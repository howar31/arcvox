<script lang="ts">
  import type { Snippet } from "svelte";
  let { tone = "neutral", children }: { tone?: "neutral" | "accent" | "ok" | "warn" | "live"; children: Snippet } = $props();
</script>

<span class="tag {tone}">{@render children()}</span>

<style>
  .tag { display: inline-flex; align-items: center; gap: 5px; height: 22px; padding: 0 9px; border-radius: 11px; font-size: var(--text-xs); font-weight: 600; background: var(--sunken); color: var(--ink-2); white-space: nowrap; vertical-align: middle; flex: none; }
  .accent { background: var(--accent-soft); color: var(--accent); }
  .ok { color: var(--ok); }
  .warn { color: var(--warn); }
  .live { color: var(--live); }
</style>
