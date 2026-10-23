<script lang="ts" module>
  import type { HistoryTag } from "../../../lib/api";
  /** Which rows the History page shows: all, those with a recording, or those carrying one tag. */
  export type Shown = "all" | "audio" | HistoryTag;
</script>

<script lang="ts">
  import { HISTORY_TAGS } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import Icon from "../../../lib/ui/Icon.svelte";

  /** A button that names the rows shown and opens a list to pick another kind; a pick closes it. */
  let { value, onchange }: { value: Shown; onchange: (v: Shown) => void } = $props();

  let open = $state(false);

  const labelOf = (v: Shown): string => {
    switch (v) {
      case "all": return t("history.filter.all");
      case "audio": return t("history.filter.with_audio");
      case "cancelled": return t("history.cancelled_tag");
      case "failed": return t("history.failed_tag");
      case "not_rewritten": return t("history.not_rewritten");
      case "corrected": return t("history.corrected");
      case "redone": return t("history.redone");
    }
  };
  const groups: Shown[][] = [["all", "audio"], [...HISTORY_TAGS]];

  function pick(v: Shown) {
    open = false;
    if (v !== value) onchange(v);
  }
  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && open) { e.preventDefault(); open = false; }
  }
</script>

<svelte:window onkeydown={onkeydown} />

<div class="picker">
  <button type="button" class="pbtn" class:set={value !== "all"} aria-expanded={open} aria-label={t("history.filter.label")} onclick={() => (open = !open)}>
    <span>{t("history.filter.button")}：{labelOf(value)}</span><Icon name="chevron-down" size={12} />
  </button>
  {#if open}
    <div class="pop" role="menu" aria-label={t("history.filter.label")}>
      {#each groups as group, i (i)}
        {#if i > 0}<hr />{/if}
        {#each group as v (v)}
          <button type="button" role="menuitemradio" aria-checked={v === value} class:on={v === value} onclick={() => pick(v)}>
            <span class="mark">{#if v === value}<Icon name="check" size={13} />{/if}</span>{labelOf(v)}
          </button>
        {/each}
      {/each}
    </div>
  {/if}
</div>

<style>
  .picker { position: relative; }
  .pbtn { display: inline-flex; align-items: center; gap: 8px; height: var(--control); padding: 0 12px 0 14px; border-radius: var(--radius); border: 1px solid var(--line-strong); background: var(--surface); color: var(--ink); font-size: var(--text-control); white-space: nowrap; cursor: default; }
  .pbtn.set { border-color: var(--accent); color: var(--accent); font-weight: 600; }
  .pbtn :global(svg) { color: var(--ink-3); }
  .pop { position: absolute; top: calc(100% + 8px); right: 0; z-index: 20; display: flex; flex-direction: column; min-width: 184px; padding: 6px; border-radius: 14px; border: 1px solid var(--line); background: var(--surface); box-shadow: 0 10px 30px rgba(0,0,0,.14), var(--lift); color: var(--ink); }
  .pop button { display: flex; align-items: center; gap: 8px; height: 32px; padding: 0 10px 0 6px; border: 0; border-radius: 8px; background: none; color: var(--ink); font-size: var(--text-control); text-align: left; white-space: nowrap; cursor: default; }
  .pop button:hover { background: var(--sunken); }
  .pop button.on { font-weight: 600; }
  .mark { width: 18px; flex: none; display: inline-grid; place-items: center; color: var(--accent); }
  hr { border: 0; border-top: 1px solid var(--line); margin: 5px 4px; }
</style>
