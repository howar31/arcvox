<script lang="ts">
  import { t } from "../i18n";
  let { dirty, saving = false, onsave, onrevert, message }: {
    dirty: boolean;
    saving?: boolean;
    onsave: () => void;
    onrevert: () => void;
    /** Replaces the default "unsaved changes" line, e.g. with a validation error. */
    message?: string;
  } = $props();
</script>

{#if dirty}
  <div class="savebar" role="region" aria-label={t("common.unsaved")}>
    <span class="message">{message ?? t("common.unsaved")}</span>
    <button type="button" class="btn" disabled={saving} onclick={onrevert}>{t("common.revert")}</button>
    <button type="button" class="btn primary" disabled={saving} onclick={onsave}>{t("common.save")}</button>
  </div>
{/if}

<style>
  .savebar { position: sticky; bottom: 0; z-index: 5; margin: 24px -32px -80px; padding: 12px 32px; display: flex; align-items: center; gap: 10px; background: color-mix(in srgb, var(--page) 88%, transparent); backdrop-filter: blur(12px); -webkit-backdrop-filter: blur(12px); border-top: 1px solid var(--line); }
  .message { flex: 1; min-width: 0; color: var(--ink-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
