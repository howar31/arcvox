<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  let { onremove, children }: { onremove?: () => void; children: Snippet } = $props();
</script>

<span class="chip" class:removable={!!onremove}>
  <span class="text">{@render children()}</span>
  {#if onremove}
    <button type="button" onclick={onremove} aria-label={t("common.remove")}><Icon name="x" size={10} /></button>
  {/if}
</span>

<style>
  .chip { display: inline-flex; align-items: center; gap: 4px; height: 24px; padding: 0 10px; border-radius: 12px; background: var(--sunken); border: 1px solid var(--line); font-size: 12px; max-width: 100%; }
  .chip.removable { padding-right: 4px; }
  .text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  button { display: inline-flex; align-items: center; justify-content: center; width: 18px; height: 18px; padding: 0; border: 0; border-radius: 50%; background: none; color: var(--ink-3); cursor: default; flex: none; }
  button:hover { background: var(--line); color: var(--ink); }
</style>
