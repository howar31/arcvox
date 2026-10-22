<script lang="ts">
  import { appIcon, ensureAppIcons } from "../apps.svelte";

  /** An application's icon at `size` px; a blank square of the same size while it loads or when there is none. */
  let { bundleId, size = 20 }: { bundleId: string; size?: number } = $props();
  $effect(() => { ensureAppIcons([bundleId]); });
  const src = $derived(appIcon(bundleId));
</script>

{#if src}
  <img class="app-icon" {src} alt="" width={size} height={size} draggable="false" />
{:else}
  <span class="app-icon blank" style:width="{size}px" style:height="{size}px"></span>
{/if}

<style>
  .app-icon { flex: none; display: inline-block; vertical-align: middle; }
  .blank { border-radius: 22%; background: color-mix(in srgb, var(--ink) 6%, transparent); }
</style>
