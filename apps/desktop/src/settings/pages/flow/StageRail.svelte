<script lang="ts">
  /** The nine stages as a column of cells: name, current setting, what it did last time. A chevron under the number column leads to the next cell. */
  import { tip } from "../../../lib/tip";
  import type { StageId } from "../../../lib/flowLast";
  import Bang from "../../../lib/ui/Bang.svelte";

  export interface Cell {
    id: StageId;
    name: string;
    /** The current setting, one line. */
    setting: string;
    /** What the stage did in the last run; "" before the first dictation. */
    line: string;
    /** How long the stage took in the last run; null for a stage without a stored time. */
    time: string | null;
    /** False when the profile does not run the stage: the cell is dimmed. */
    open: boolean;
    /** The setting names something missing (a model not downloaded, an engine gone). */
    warn: boolean;
    unknown: boolean;
    /** Set when the stage runs on another engine than the one the profile names: the problem mark, with this text. */
    problem?: string;
  }

  let { cells, selected, onpick, label }: { cells: Cell[]; selected: StageId; onpick: (id: StageId) => void; label: string } = $props();
</script>

<div class="rail" role="tablist" aria-orientation="vertical" aria-label={label}>
  {#each cells as c, i (c.id)}
    <button type="button" class="blk" class:on={selected === c.id} class:off={!c.open} role="tab" aria-selected={selected === c.id} onclick={() => onpick(c.id)}>
      <span class="num">{i + 1}</span>
      <span class="t">
        <b>{c.name}{#if c.problem}<Bang tone="warn" title={c.problem} />{/if}</b>
        <span class="setting" class:warn={c.warn && c.open} class:unknown={c.unknown} use:tip={c.setting}>{c.setting}</span>
        <!-- The last-run line carries the stage's time, faint, at the cell's right; the name row stays whole. -->
        <span class="last"><i>{c.line}</i>{#if c.time}<em class="time">{c.time}</em>{/if}</span>
      </span>
    </button>
  {/each}
</div>

<style>
  .rail { --sel: color-mix(in srgb, var(--accent) 10%, var(--surface)); background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); overflow: visible; }
  :global(:root[data-theme="dark"]) .rail { --sel: color-mix(in srgb, var(--accent) 16%, var(--surface)); }
  @media (prefers-color-scheme: dark) { :global(:root:not([data-theme="light"])) .rail { --sel: color-mix(in srgb, var(--accent) 16%, var(--surface)); } }
  .blk { position: relative; display: grid; grid-template-columns: 26px 1fr; gap: 10px; align-items: start; width: 100%; padding: 9px 12px 9px 14px; border: 0; border-top: 1px solid var(--line); background: none; text-align: left; cursor: default; min-width: 0; }
  .blk:first-child { border-top: 0; border-radius: var(--radius-group) var(--radius-group) 0 0; }
  .blk:last-child { border-radius: 0 0 var(--radius-group) var(--radius-group); }
  .blk:hover { background: color-mix(in srgb, var(--sunken) 60%, transparent); }
  .blk.on { background: var(--sel); }
  .blk:focus-visible { outline-offset: -2px; }
  /* The chevron under the number column, riding on the dividing line to the next cell: a line-coloured
     triangle (the V) with a smaller one in the cell's own colour inside it. Two clipped triangles rather
     than a rotated square: the square's edges inside the cell showed as a diamond in the app. */
  .blk:not(:last-child)::before, .blk:not(:last-child)::after { content: ""; position: absolute; left: 27px; transform: translateX(-50%); clip-path: polygon(0 0, 100% 0, 50% 100%); z-index: 1; pointer-events: none; }
  .blk:not(:last-child)::before { bottom: -8px; width: 14px; height: 8px; background: var(--line); }
  .blk:not(:last-child)::after { bottom: -6px; width: 11px; height: 6.5px; background: var(--surface); }
  /* The inside is the cell's own colour in every state, so the cell and its chevron read as one shape. */
  .blk:hover:not(:last-child)::after { background: color-mix(in srgb, var(--sunken) 60%, var(--surface)); }
  .blk.on:not(:last-child)::after { background: var(--sel); }
  .num { width: 26px; height: 26px; margin-top: 1px; border-radius: 13px; background: var(--sunken); color: var(--ink-2); font: 600 13px/26px var(--font); text-align: center; }
  .blk.on .num { background: var(--accent); color: var(--accent-ink); }
  .blk.off .num { background: transparent; border: 1px dashed var(--line-strong); color: var(--ink-3); line-height: 24px; }
  .blk.off.on .num { background: var(--accent); border: 0; color: var(--accent-ink); line-height: 26px; }
  .t { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .t b { display: flex; align-items: center; gap: 6px; font-weight: 600; font-size: 15px; line-height: 1.35; }
  .last { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; min-width: 0; }
  .time { flex: none; font-style: normal; font-size: 11px; color: var(--ink-3); opacity: 0.75; white-space: nowrap; }
  .t .setting { color: var(--ink-2); font-size: var(--text-xs); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .t .setting.warn { color: var(--warn); }
  .t .setting.unknown { color: var(--live); }
  .t i { font-style: normal; color: var(--ink-3); font-size: var(--text-xs); min-height: 1.4em; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .blk.off .t b, .blk.off .t .setting, .blk.off .t i { color: var(--ink-3); }
</style>
