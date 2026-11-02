<script lang="ts" generics="T">
  /** A list whose rows are ticked to act on several at once: a press on a row or its box ticks it, Shift ticks the run from the last pressed row, the box above ticks every row. */
  import type { Snippet } from "svelte";
  import { t } from "../i18n";
  import { coverage, pick, pickAll } from "../pickList";
  import { tip } from "../tip";

  let { items, key, selected = $bindable([]), locked, row, label, busy = false }: {
    items: T[];
    key: (item: T) => string;
    selected?: string[];
    /** Why a row cannot be ticked; null when it can. */
    locked?: (item: T) => string | null;
    /** What the row shows after its box; the second value says whether it is ticked. */
    row: Snippet<[T, boolean]>;
    /** Accessible name of a row's box. */
    label: (item: T) => string;
    busy?: boolean;
  } = $props();

  let anchor = $state<string | null>(null);
  const order = $derived(items.map(key));
  const isLocked = $derived((id: string) => {
    const item = items.find((x) => key(x) === id);
    return !!item && !!locked?.(item);
  });
  const all = $derived(coverage(order, selected, isLocked));

  function press(item: T, shift: boolean) {
    const next = pick(order, { selected, anchor }, key(item), shift, isLocked);
    selected = next.selected;
    anchor = next.anchor;
  }
  function onrow(e: MouseEvent, item: T) {
    // Buttons and fields inside the row do their own thing.
    if ((e.target as HTMLElement).closest("button, input, a, [role=button]")) return;
    press(item, e.shiftKey);
  }
</script>

<div class="list" class:busy inert={busy} aria-busy={busy}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="row head" onclick={(e) => { if (!(e.target as HTMLElement).closest("button")) selected = pickAll(order, selected, isLocked); }}>
    <button type="button" class="cb" class:on={all === "all"} class:some={all === "some"} role="checkbox" aria-checked={all === "all" ? true : all === "some" ? "mixed" : false} aria-label={t("common.select_all")} onclick={() => (selected = pickAll(order, selected, isLocked))}></button>
    <span class="all">{t("common.select_all")}</span>
  </div>
  {#each items as item (key(item))}
    {@const id = key(item)}
    {@const on = selected.includes(id)}
    {@const why = locked?.(item) ?? null}
    <!-- The row picks too; the box stays the control for the keyboard. Shift keeps the text from being selected. -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="row" class:on class:pickable={!why} onmousedown={(e) => { if (e.shiftKey) e.preventDefault(); }} onclick={(e) => onrow(e, item)}>
      <button type="button" class="cb" class:on role="checkbox" aria-checked={on} aria-label={label(item)} disabled={!!why} use:tip={why ?? undefined} onclick={(e) => press(item, e.shiftKey)}></button>
      <div class="body">{@render row(item, on)}</div>
    </div>
  {/each}
</div>

<style>
  .list { border: 1px solid var(--line); border-radius: var(--radius-group); background: var(--surface); color: var(--ink); }
  .list.busy { opacity: .55; }
  .row { display: grid; grid-template-columns: 18px minmax(0, 1fr); gap: 14px; align-items: center; padding: 12px 16px; border-top: 1px solid var(--line); }
  .row:first-child { border-top: 0; }
  .row.head { padding-top: 10px; padding-bottom: 10px; }
  .all { color: var(--ink-2); font-size: var(--text-sm); }
  .row.on { background: var(--accent-soft); }
  .row.pickable:hover, .row.head:hover { background: color-mix(in srgb, var(--sunken) 60%, transparent); }
  .row.pickable.on:hover { background: var(--accent-soft); }
  .cb { width: 18px; height: 18px; padding: 0; border: 1.5px solid var(--line-strong); border-radius: 5px; background: var(--surface); position: relative; cursor: default; }
  .cb.on, .cb.some { background: var(--accent); border-color: var(--accent); }
  .cb.on::after { content: ""; position: absolute; left: 5px; top: 1px; width: 5px; height: 10px; border-right: 2px solid var(--accent-ink); border-bottom: 2px solid var(--accent-ink); transform: rotate(45deg); }
  .cb.some::after { content: ""; position: absolute; left: 3px; top: 7px; width: 9px; height: 2px; border-radius: 1px; background: var(--accent-ink); }
  .cb:disabled { opacity: .35; }
  .body { min-width: 0; }
</style>
