<script lang="ts">
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  import { toast } from "./toast.svelte";
</script>

<div class="toasts" aria-live="polite">
  {#each toast.items as item (item.id)}
    <div class="toast" class:error={item.kind === "error"} role={item.kind === "error" ? "alert" : "status"}>
      <span class="glyph"><Icon name={item.kind === "error" ? "x" : "check"} size={18} /></span>
      <span class="message">{item.message}</span>
      {#if item.kind === "error"}
        <button type="button" onclick={() => toast.dismiss(item.id)} aria-label={t("common.dismiss")}><Icon name="x" size={12} /></button>
      {/if}
    </div>
  {/each}
</div>

<style>
  /* Centred under the page, clear of the sidebar; above the save bar while it shows. */
  .toasts { position: fixed; bottom: 26px; left: var(--sidebar-w); right: 0; z-index: 60; display: flex; flex-direction: column; align-items: center; gap: 8px; padding: 0 24px; pointer-events: none; }
  :global(body:has(.savebar)) .toasts { bottom: 78px; }
  .toast { pointer-events: auto; display: flex; align-items: center; gap: 10px; max-width: min(520px, 100%); min-height: 46px; padding: 9px 20px 9px 16px; background: var(--ink); color: var(--page); border-radius: 23px; box-shadow: var(--shadow); font-weight: 600; animation: enter .3s cubic-bezier(.2, .9, .3, 1.25); }
  .glyph { display: inline-flex; flex: none; color: color-mix(in srgb, var(--ok) 65%, var(--page)); }
  .toast.error { background: var(--live); color: #fff; padding-right: 10px; }
  .toast.error .glyph { color: inherit; }
  .message { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  button { display: inline-flex; align-items: center; justify-content: center; width: 26px; height: 26px; padding: 0; border: 0; border-radius: 50%; background: none; color: inherit; opacity: .75; flex: none; cursor: default; }
  button:hover { background: rgba(255, 255, 255, .2); opacity: 1; }
  @keyframes enter { from { opacity: 0; transform: translateY(24px) scale(.94); } }
</style>
