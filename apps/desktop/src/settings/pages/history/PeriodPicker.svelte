<script lang="ts">
  import { tip } from "../../../lib/tip";
  import { errorMessage, historyDays, historyHours, type DayCount, type HistoryTag } from "../../../lib/api";
  import { periodLabel, periodRange, type PeriodSel } from "../../../lib/historyView";
  import { t } from "../../../lib/i18n";
  import Icon from "../../../lib/ui/Icon.svelte";
  import { toast } from "../../../lib/ui/toast.svelte";

  /**
   * A button that names the period shown and opens a popover to change it: quick choices, a calendar
   * whose days are shaded by how many rows they hold (a day, or two for a range), and for a single
   * day a row of hours. The popover stays open until Done, Escape, or the button is pressed again;
   * every change is applied at once through `onchange`.
   */
  let { value, withAudio = false, tag = null, onchange }: { value: PeriodSel; withAudio?: boolean; tag?: HistoryTag | null; onchange: (v: PeriodSel) => void } = $props();

  let open = $state(false);
  let days = $state<Record<string, number>>({});
  let hours = $state<number[]>([]);
  // The month the calendar shows.
  let view = $state({ year: new Date().getFullYear(), month: new Date().getMonth() });
  // While a range is being picked: the first day chosen, waiting for the second.
  let first = $state<string | null>(null);
  // While hours are being dragged over.
  let dragging = $state(false);

  const tz = () => -new Date().getTimezoneOffset() * 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  const key = (y: number, m: number, d: number) => `${y}-${pad(m + 1)}-${pad(d)}`;
  const quick: PeriodSel["kind"][] = ["all", "today", "yesterday", "7d", "30d"];
  const weekdays = $derived(t("history.period.weekdays").split(","));
  const singleDay = $derived(value.kind === "range" && !!value.from && value.from === value.to ? value.from : null);
  const label = $derived(periodLabel(value));
  const chosen = $derived(value.kind !== "all" && !(value.kind === "range" && !value.from && !value.to));

  async function loadDays() {
    try {
      const got = await historyDays(tz(), withAudio, tag);
      days = Object.fromEntries(got.map((d: DayCount) => [d.day, d.count]));
    } catch (e) {
      toast.error(errorMessage(e));
    }
  }
  async function loadHours(day: string) {
    const { since, until } = periodRange({ kind: "range", from: day, to: day }, new Date());
    if (since === null || until === null) return;
    try { hours = await historyHours(since, until, tz(), withAudio, tag); } catch (e) { toast.error(errorMessage(e)); }
  }
  $effect(() => {
    if (!open) return;
    loadDays();
    if (singleDay) loadHours(singleDay);
    else hours = [];
  });
  $effect(() => {
    // The month of the selection when it opens.
    if (open && value.kind === "range" && value.from) {
      const [y, m] = value.from.split("-").map(Number);
      view = { year: y, month: m - 1 };
    }
  });

  function toggle() {
    open = !open;
    first = null;
  }
  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && open) { e.preventDefault(); open = false; }
  }
  function pickQuick(kind: PeriodSel["kind"]) {
    first = null;
    onchange({ kind });
  }
  function pickDay(k: string) {
    if (first === null) {
      first = k;
      onchange({ kind: "range", from: k, to: k });
    } else {
      const [from, to] = first <= k ? [first, k] : [k, first];
      first = null;
      onchange({ kind: "range", from, to });
    }
  }
  function pickHour(h: number, extend: boolean) {
    if (!singleDay) return;
    const from = extend && value.hourFrom !== undefined ? value.hourFrom : h;
    onchange({ kind: "range", from: singleDay, to: singleDay, hourFrom: from, hourTo: h });
  }
  function clearHours() {
    if (singleDay) onchange({ kind: "range", from: singleDay, to: singleDay });
  }
  function clear() {
    first = null;
    onchange({ kind: "all" });
  }
  function shift(months: number) {
    const d = new Date(view.year, view.month + months, 1);
    view = { year: d.getFullYear(), month: d.getMonth() };
  }

  /** The cells of the month shown: leading days of the previous month, then the month's days. */
  const cells = $derived.by(() => {
    const firstDay = new Date(view.year, view.month, 1);
    const lead = (firstDay.getDay() + 6) % 7;
    const count = new Date(view.year, view.month + 1, 0).getDate();
    const max = Math.max(1, ...Object.values(days));
    const out: { key: string; day: number; off: boolean; n: number; level: number }[] = [];
    for (let i = lead; i > 0; i--) {
      const d = new Date(view.year, view.month, 1 - i);
      out.push({ key: key(d.getFullYear(), d.getMonth(), d.getDate()), day: d.getDate(), off: true, n: 0, level: 0 });
    }
    for (let d = 1; d <= count; d++) {
      const k = key(view.year, view.month, d);
      const n = days[k] ?? 0;
      out.push({ key: k, day: d, off: false, n, level: n === 0 ? 0 : n < max * 0.25 ? 1 : n < max * 0.6 ? 2 : 3 });
    }
    return out;
  });
  const from = $derived(value.kind === "range" ? (first ?? value.from ?? "") : "");
  const to = $derived(value.kind === "range" ? (first ?? value.to ?? "") : "");
  const hourSpan = $derived.by(() => {
    if (!singleDay || value.hourFrom === undefined) return null;
    const b = value.hourTo ?? value.hourFrom;
    return [Math.min(value.hourFrom, b), Math.max(value.hourFrom, b)] as const;
  });
  const hourMax = $derived(Math.max(1, ...hours));
  const hourRows = $derived.by(() => {
    if (!hours.length) return 0;
    const [a, b] = hourSpan ?? [0, 23];
    return hours.slice(a, b + 1).reduce((x, y) => x + y, 0);
  });
  const monthTitle = $derived(new Date(view.year, view.month, 1).toLocaleDateString(t("history.period.locale"), { year: "numeric", month: "long" }));
</script>

<svelte:window onkeydown={onkeydown} onmouseup={() => (dragging = false)} />

<div class="picker">
  <button type="button" class="pbtn" class:set={chosen} aria-expanded={open} onclick={toggle}>
    <span>{t("history.period.label")}：{label}</span><Icon name="chevron-down" size={12} />
  </button>
  {#if open}
    <div class="pop" role="dialog" aria-label={t("history.period.label")}>
      <div class="quick">
        {#each quick as kind (kind)}
          <button type="button" class:on={value.kind === kind} onclick={() => pickQuick(kind)}>{t(`history.period.${kind}`)}</button>
        {/each}
      </div>
      <div class="mh">
        <span class="title">{monthTitle}</span>
        <span class="nav">
          <button type="button" aria-label={t("history.period.prev")} onclick={() => shift(-1)}><Icon name="chevron-left" size={14} /></button>
          <button type="button" aria-label={t("history.period.next")} onclick={() => shift(1)}><Icon name="chevron-right" size={14} /></button>
        </span>
      </div>
      <div class="grid">
        {#each weekdays as w (w)}<span class="wd">{w}</span>{/each}
        {#each cells as c (c.key)}
          <button
            type="button" class="d" class:off={c.off} class:none={!c.off && c.n === 0} class:l1={c.level === 1} class:l2={c.level === 2} class:l3={c.level === 3}
            class:a={from && c.key === from} class:b={to && c.key === to} class:in={from && to && c.key > from && c.key < to}
            onclick={() => pickDay(c.key)}
          >{c.day}{#if c.n && !c.off}<span class="n">{c.n}</span>{/if}</button>
        {/each}
      </div>
      {#if singleDay && hours.length}
        <div class="hours">
          <div class="lab"><span>{t("history.period.hours")} <b>{periodLabel({ kind: "range", from: singleDay, to: singleDay })}</b></span><span>{t("history.period.rows", { n: String(hourRows) })}</span></div>
          <div class="hrow" role="group" aria-label={t("history.period.hours")}>
            {#each hours as n, h (h)}
              <button
                type="button" class="h" class:l1={n > 0 && n < hourMax * 0.25} class:l2={n >= hourMax * 0.25 && n < hourMax * 0.6} class:l3={n >= hourMax * 0.6}
                class:in={hourSpan && h >= hourSpan[0] && h <= hourSpan[1]} use:tip={`${h}:00 ${t("history.period.rows", { n: String(n) })}`} aria-label={`${h}:00 ${t("history.period.rows", { n: String(n) })}`}
                onmousedown={(e) => { e.preventDefault(); dragging = true; pickHour(h, e.shiftKey); }}
                onmouseenter={() => { if (dragging) pickHour(h, true); }}
              ></button>
            {/each}
          </div>
          <div class="ticks">{#each hours as _, h (h)}<span class:shown={h % 3 === 0}>{h}</span>{/each}</div>
        </div>
      {/if}
      <div class="foot">
        <span class="text">{chosen ? label : t("history.period.none")}</span>
        <span class="sp"></span>
        {#if hourSpan}<button type="button" class="btn quiet sm" onclick={clearHours}>{t("history.period.all_hours")}</button>{/if}
        <button type="button" class="btn quiet sm" onclick={clear}>{t("history.range.clear")}</button>
        <button type="button" class="btn primary sm" onclick={() => (open = false)}>{t("history.period.done")}</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .picker { position: relative; }
  .pbtn { display: inline-flex; align-items: center; gap: 8px; height: var(--control); padding: 0 12px 0 14px; border-radius: var(--radius); border: 1px solid var(--line-strong); background: var(--surface); color: var(--ink); font-size: var(--text-control); white-space: nowrap; cursor: default; }
  .pbtn.set { border-color: var(--accent); color: var(--accent); font-weight: 600; }
  .pbtn :global(svg) { color: var(--ink-3); }
  .pop { position: absolute; top: calc(100% + 8px); right: 0; z-index: 20; width: max-content; min-width: 352px; max-width: 560px; padding: 14px 14px 12px; border-radius: 16px; border: 1px solid var(--line); background: var(--surface); box-shadow: 0 10px 30px rgba(0,0,0,.14), var(--lift); color: var(--ink); font-weight: 400; }
  /* One line in every language the width allows; a longer one wraps. */
  .quick { display: flex; gap: 5px; flex-wrap: wrap; margin-bottom: 12px; }
  .quick button { height: 28px; padding: 0 10px; white-space: nowrap; border-radius: 14px; border: 1px solid var(--line-strong); background: none; font-size: var(--text-xs); color: var(--ink-2); cursor: default; }
  .quick button.on { background: var(--accent-soft); border-color: var(--accent); color: var(--accent); font-weight: 600; }
  .mh { display: flex; align-items: center; justify-content: space-between; margin: 2px 2px 8px; font-size: var(--text-control); font-weight: 600; }
  .nav { display: inline-flex; gap: 4px; }
  .nav button { width: 28px; height: 28px; display: inline-flex; align-items: center; justify-content: center; border: 0; border-radius: 8px; background: none; color: var(--ink-2); cursor: default; }
  .nav button:hover { background: var(--sunken); }
  .grid { display: grid; grid-template-columns: repeat(7, 1fr); gap: 2px; }
  .wd { text-align: center; font-size: var(--text-xs); color: var(--ink-3); padding: 2px 0 6px; }
  .d { position: relative; height: 40px; display: flex; flex-direction: column; align-items: center; justify-content: center; border: 0; border-radius: 9px; background: none; font-size: var(--text-sm); font-variant-numeric: tabular-nums; color: var(--ink); cursor: default; }
  .d.off { color: var(--ink-3); opacity: .55; }
  .d.none { color: var(--ink-3); }
  .d .n { font-size: 10px; line-height: 1; color: var(--ink-3); margin-top: 2px; }
  .d.l1 { background: color-mix(in srgb, var(--accent) 8%, transparent); }
  .d.l2 { background: color-mix(in srgb, var(--accent) 18%, transparent); }
  .d.l3 { background: color-mix(in srgb, var(--accent) 32%, transparent); }
  /* The count stays a fill; the selection is a ring, so the two never look alike. */
  .d.in { box-shadow: inset 0 0 0 2px var(--accent); }
  .d.a, .d.b { background: var(--accent); color: var(--accent-ink); }
  .d.a .n, .d.b .n { color: var(--accent-ink); opacity: .8; }
  .d:hover:not(.a):not(.b) { box-shadow: inset 0 0 0 1.5px var(--ink-3); }
  .hours { margin-top: 12px; padding-top: 12px; border-top: 1px solid var(--line); }
  .lab { display: flex; justify-content: space-between; font-size: var(--text-xs); font-weight: 600; color: var(--ink-2); margin-bottom: 6px; }
  .lab b { color: var(--ink); font-weight: 600; }
  .hrow { display: grid; grid-template-columns: repeat(24, 1fr); gap: 2px; }
  .h { height: 30px; border: 0; border-radius: 5px; background: var(--sunken); padding: 0; cursor: default; }
  .h.l1 { background: color-mix(in srgb, var(--accent) 18%, transparent); }
  .h.l2 { background: color-mix(in srgb, var(--accent) 36%, transparent); }
  .h.l3 { background: color-mix(in srgb, var(--accent) 60%, transparent); }
  .h.in { background: var(--accent); }
  .ticks { display: grid; grid-template-columns: repeat(24, 1fr); font-size: 9.5px; color: var(--ink-3); margin-top: 3px; }
  .ticks span { visibility: hidden; }
  .ticks span.shown { visibility: visible; }
  .foot { display: flex; align-items: center; gap: 4px; margin-top: 12px; font-size: var(--text-sm); color: var(--ink-2); }
  .foot .text { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .foot .sp { flex: 1; }
  .btn.sm { height: 30px; padding: 0 12px; font-size: var(--text-sm); }
</style>
