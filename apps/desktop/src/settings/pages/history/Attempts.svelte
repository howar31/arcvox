<script lang="ts">
  /**
   * The results a dictation had, one line each: the one in use first and shown whole, the others folded to
   * one line until pressed. Beyond a few, the rest wait behind a button.
   */
  import type { Attempt } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import Icon from "../../../lib/ui/Icon.svelte";
  import Tag from "../../../lib/ui/Tag.svelte";

  let { items, fresh = null, when, profileName, engines, timing, failure, oncopy, onuse, ondelete }: {
    /** Newest first; the original result is the oldest. */
    items: Attempt[];
    /** The result just made, which waits to be chosen. */
    fresh?: number | null;
    when: (a: Attempt) => string;
    profileName: (id: string) => string;
    engines: (asr: string, llm: string | null) => string;
    timing: (a: Attempt) => string | null;
    failure: (a: Attempt) => string;
    oncopy: (text: string) => void;
    onuse: (a: Attempt) => void;
    ondelete: (a: Attempt) => void;
  } = $props();

  // How many results show before the rest wait behind a button.
  const SHOWN = 5;

  // The result opened by a press; the one in use is always open.
  let expanded = $state<number | null>(null);
  let all = $state(false);
  // The result whose delete button was pressed once; a second press within a few seconds deletes it.
  let confirming = $state<number | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;

  const ordered = $derived([...items.filter((a) => a.chosen), ...items.filter((a) => !a.chosen)]);
  const shown = $derived(all || ordered.length <= SHOWN ? ordered : ordered.slice(0, SHOWN));
  const isOpen = (a: Attempt) => a.chosen || expanded === a.id;

  // A new result opens by itself, so it is read before it is chosen.
  $effect(() => { if (fresh !== null) expanded = fresh; });

  function askDelete(a: Attempt) {
    clearTimeout(timer);
    confirming = a.id;
    timer = setTimeout(() => { if (confirming === a.id) confirming = null; }, 4000);
  }
  function remove(a: Attempt) {
    clearTimeout(timer);
    confirming = null;
    ondelete(a);
  }
</script>

<div class="list">
  {#each shown as a (a.id)}
    {@const open = isOpen(a)}
    <div class="attempt" class:open class:chosen={a.chosen} class:fresh={fresh === a.id}>
      <button type="button" class="main" aria-expanded={open} aria-label={t("history.attempt_show")} disabled={a.chosen} onclick={() => (expanded = expanded === a.id ? null : a.id)}>
        <span class="dot"></span>
        <span class="body">
          <span class="head num">
            <span class="when">{when(a)}</span>
            <span>{profileName(a.profile_id)}</span>
            {#if a.asr_label}<span>{engines(a.asr_label, a.llm_label)}</span>{/if}
            {#if a.error === null && timing(a)}<span>{timing(a)}</span>{/if}
            {#if a.chosen}<Tag tone="accent">{t("history.attempt_chosen")}</Tag>{/if}
            {#if fresh === a.id}<Tag tone="accent">{t("history.attempt_fresh")}</Tag>{/if}
            {#if a.origin}<Tag>{t("history.attempt_origin")}</Tag>{/if}
            {#if a.error !== null}<Tag tone="live">{t("history.attempt_failed")}</Tag>{/if}
            {#if a.degraded}<Tag tone="warn">{t("history.not_rewritten")}</Tag>{/if}
          </span>
          {#if a.error !== null}
            <span class="text problem">{failure(a)}</span>
          {:else}
            <span class="text">{a.final_text}</span>
          {/if}
        </span>
      </button>
      {#if !a.chosen && !a.origin}
        {#if confirming === a.id}
          <button type="button" class="btn sm danger sure" onclick={() => remove(a)}>{t("history.delete_attempt_confirm")}</button>
        {:else}
          <button type="button" class="x" aria-label={t("history.delete_attempt")} title={t("history.delete_attempt")} onclick={() => askDelete(a)}><Icon name="x" size={12} /></button>
        {/if}
      {/if}
      {#if open && !a.chosen && a.error === null}
        <div class="acts">
          <button type="button" class="btn sm" onclick={() => oncopy(a.final_text)}>{t("history.attempt_copy")}</button>
          <button type="button" class="btn sm primary" onclick={() => onuse(a)}>{t("history.use_result")}</button>
        </div>
      {/if}
    </div>
  {/each}
  {#if !all && ordered.length > SHOWN}
    <button type="button" class="more" onclick={() => (all = true)}>{t("history.attempts_more", { n: String(ordered.length - SHOWN) })}</button>
  {/if}
</div>

<style>
  .list { display: flex; flex-direction: column; }
  .attempt { position: relative; display: flex; flex-wrap: wrap; align-items: flex-start; gap: 0 8px; padding: 9px 0; border-top: 1px solid var(--line); }
  .attempt:first-child { border-top: 0; }
  .attempt.open:not(.chosen) { margin: 0 -16px; padding-left: 16px; padding-right: 16px; border-radius: 10px; background: var(--sunken); }
  .attempt.open:not(.chosen) + .attempt { border-top-color: transparent; }
  .main { flex: 1; min-width: 0; display: flex; align-items: flex-start; gap: 10px; padding: 0; border: 0; background: none; color: var(--ink); text-align: left; font: inherit; cursor: default; }
  .main:focus-visible { outline-offset: 2px; border-radius: 6px; }
  .dot { width: 16px; height: 16px; flex: none; margin-top: 3px; border-radius: 50%; border: 1.5px solid var(--line-strong); }
  .chosen .dot { border: 5px solid var(--accent); }
  .fresh .dot { border-color: var(--accent); border-style: dashed; }
  .body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .head { display: flex; align-items: center; flex-wrap: wrap; gap: 2px 12px; color: var(--ink-2); font-size: var(--text-sm); }
  .when { color: var(--ink); font-weight: 600; }
  .text { line-height: 1.55; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .open .text { overflow: visible; white-space: pre-line; overflow-wrap: anywhere; -webkit-user-select: text; user-select: text; }
  .problem { color: var(--live); }
  .x { flex: none; display: inline-flex; align-items: center; justify-content: center; width: 28px; height: 28px; margin-top: -2px; padding: 0; border: 0; border-radius: 8px; background: none; color: var(--ink-3); cursor: default; }
  .x:hover { background: var(--sunken); color: var(--ink); }
  .sure { flex: none; height: 28px; margin-top: -2px; padding: 0 10px; font-size: var(--text-xs); }
  .acts { flex: 1 0 100%; display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; }
  .more { align-self: flex-start; margin: 6px 0 0 26px; padding: 0; border: 0; background: none; color: var(--accent); font-size: var(--text-control); cursor: default; }
  .more:hover { text-decoration: underline; }
</style>
