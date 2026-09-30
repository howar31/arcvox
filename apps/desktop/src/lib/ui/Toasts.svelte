<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import Ring from "./Ring.svelte";
  import { toast } from "./toast.svelte";
</script>

<div class="toasts" aria-live="polite">
  {#each toast.items as item (item.id)}
    <div class="toast" class:error={item.kind === "error"} role={item.kind === "error" ? "alert" : "status"}>
      <Ring tone={item.kind === "error" ? "live" : "ok"} size="sm" />
      <span class="message">{item.message}</span>
      <button type="button" onclick={() => toast.dismiss(item.id)} aria-label={t("common.dismiss")}><Icon name="x" size={12} /></button>
    </div>
  {/each}
</div>

<style>
  .toasts { position: fixed; top: 12px; right: 16px; z-index: 60; display: flex; flex-direction: column; align-items: flex-end; gap: 8px; max-width: min(360px, calc(100vw - 32px)); pointer-events: none; }
  .toast { pointer-events: auto; display: flex; align-items: center; gap: 10px; min-height: 36px; padding: 6px 6px 6px 12px; background: var(--surface); border: 1px solid var(--line); border-radius: 8px; box-shadow: var(--shadow); animation: enter .15s ease-out; }
  .toast.error { border-color: color-mix(in srgb, var(--live) 40%, var(--line)); }
  .message { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  button { display: inline-flex; align-items: center; justify-content: center; width: 22px; height: 22px; padding: 0; border: 0; border-radius: 5px; background: none; color: var(--ink-3); flex: none; cursor: default; }
  button:hover { background: var(--sunken); color: var(--ink); }
  @keyframes enter { from { opacity: 0; transform: translateY(-4px); } }
</style>
