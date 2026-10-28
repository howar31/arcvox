<script lang="ts">
  import { onDestroy } from "svelte";
  import { t } from "../i18n";
  import Icon from "./Icon.svelte";
  let { dirty, saving = false, canSave = true, onsave, onrevert, message }: {
    dirty: boolean;
    saving?: boolean;
    /** False keeps Save disabled (Revert stays available). */
    canSave?: boolean;
    onsave: () => void;
    onrevert: () => void;
    /** Replaces the default "unsaved changes" line, e.g. with a validation error. */
    message?: string;
  } = $props();

  /** How long the bar says the changes are saved, and how long it takes to leave after that. */
  const DONE_MS = 1800;
  const LEAVE_MS = 250;

  // The bar confirms a save where the button was pressed: it stays, says so, and leaves.
  let done = $state(false);
  let leaving = $state(false);
  let wasSaving = false;
  let timers: ReturnType<typeof setTimeout>[] = [];

  function clear() {
    timers.forEach(clearTimeout);
    timers = [];
    done = false;
    leaving = false;
  }

  $effect(() => {
    const finished = wasSaving && !saving;
    wasSaving = saving;
    if (dirty) {
      clear();
    } else if (finished) {
      clear();
      done = true;
      timers = [
        setTimeout(() => { leaving = true; }, DONE_MS),
        setTimeout(clear, DONE_MS + LEAVE_MS),
      ];
    }
  });

  onDestroy(clear);
</script>

<!-- Rests at the bottom of the page, and of the window when the page is longer. The slot is always in
     the layout, so the bar's arrival moves nothing on the page; the bar rises into it. -->
<div class="slot" aria-hidden={!dirty && !done}>
  {#if dirty || done}
    {#if dirty}
      <div class="savebar" role="region" aria-label={t("common.unsaved")}>
        <span class="message">{message ?? t("common.unsaved")}</span>
        <button type="button" class="btn" disabled={saving} onclick={onrevert}>{t("common.revert")}</button>
        <button type="button" class="btn primary" disabled={saving || !canSave} onclick={onsave}>{t("common.save")}</button>
      </div>
    {:else}
      <div class="savebar done" class:leaving role="status">
        <span class="message"><Icon name="check" size={18} />{t("common.saved")}</span>
      </div>
    {/if}
  {/if}
</div>

<style>
  /* `top` holds it at the bottom of the window when the page is shorter, `bottom` when it is longer. */
  /* The bar spans the whole area beside the sidebar; what it holds stays in line with the page. */
  .slot { --side: calc((100vw - var(--sidebar-w) - min(var(--page-max), 100vw - var(--sidebar-w))) / 2 + var(--page-x)); position: sticky; top: calc(100% - var(--control) - 49px); bottom: 0; z-index: 5; margin: 0 calc(-1 * var(--side)) calc(-1 * var(--page-bottom)); padding-top: 24px; min-height: calc(var(--control) + 49px); box-sizing: border-box; overflow: hidden; pointer-events: none; }
  .savebar { animation: rise .2s ease-out; pointer-events: auto; padding: 12px var(--side); min-height: calc(var(--control) + 25px); display: flex; align-items: center; gap: 10px; background: color-mix(in srgb, var(--page) 88%, transparent); backdrop-filter: blur(12px); -webkit-backdrop-filter: blur(12px); border-top: 1px solid var(--line); }
  .message { flex: 1; min-width: 0; color: var(--ink-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .done { background: color-mix(in srgb, var(--ok) 14%, var(--page)); border-top-color: color-mix(in srgb, var(--ok) 45%, transparent); transition: transform .25s ease-in, opacity .25s; }
  .done .message { display: flex; align-items: center; gap: 9px; color: var(--ok); font-weight: 600; }
  .done.leaving { transform: translateY(100%); opacity: 0; }
  @keyframes rise { from { transform: translateY(100%); } to { transform: none; } }
</style>
