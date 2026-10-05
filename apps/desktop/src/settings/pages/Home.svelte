<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import {
    errorMessage, formatBytes, getStats, historyStorage, listMicrophones, modelsInstalled,
    type HistoryStorage, type InstalledModel, type Stats, type StatsCard,
  } from "../../lib/api";
  import { uiLocale } from "../../lib/format";
  import { guard } from "../../lib/guard.svelte";
  import { t } from "../../lib/i18n";
  import { defaultProfile } from "../../lib/labels";
  import { go } from "../../lib/route";
  import { engineLine, micLine, outputLine } from "../../lib/stages";
  import {
    canInsertRow, canResize, cardDef, COLS, DEFAULT_LAYOUT, fits, GRID_GAP, hidden as hideCard, insertedAt, movedTo, occupant, resized, ROW_HEIGHT, rowCount,
    sameLayout, sanitized, supports, swapped, type CardId,
  } from "../../lib/statsGrid";
  import {
    change, dayDate, durationParts, fillDays, groupSums, heatLevel, heatWeeks, localDay, monthLabels, percent, periodStart, speed, streaks, topWithOthers, type Period,
  } from "../../lib/statsView";
  import { firstShortcut } from "../../lib/shortcut";
  import { client, config } from "../../lib/store.svelte";
  import BarList from "../../lib/ui/BarList.svelte";
  import Columns from "../../lib/ui/Columns.svelte";
  import Empty from "../../lib/ui/Empty.svelte";
  import Heatmap from "../../lib/ui/Heatmap.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Ring from "../../lib/ui/Ring.svelte";
  import Segmented from "../../lib/ui/Segmented.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  const PERIODS: Period[] = ["today", "week", "month", "all"];
  const PERIOD_KEY = "arcvox.stats.period";

  function storedPeriod(): Period {
    try {
      const v = localStorage.getItem(PERIOD_KEY);
      return PERIODS.includes(v as Period) ? (v as Period) : "month";
    } catch { return "month"; }
  }

  let period = $state<Period>(storedPeriod());
  let stats = $state<Stats | null>(null);
  /** Statistics from the start of the period before this one, for the comparison lines; null for "all". */
  let before = $state<Stats | null>(null);
  let failed = $state(false);
  let storage = $state<HistoryStorage | null>(null);
  let models = $state<InstalledModel[] | null>(null);
  let mics = $state<string[]>([]);
  let now = $state(new Date());

  async function load(of: Period = period) {
    now = new Date();
    const tz = -now.getTimezoneOffset() * 60;
    const since = periodStart(of, now);
    try {
      const nowSecs = Math.floor(now.getTime() / 1000);
      const [current, earlier] = await Promise.all([
        getStats(since, tz),
        of === "all" ? Promise.resolve(null) : getStats(since - (nowSecs - since), tz),
      ]);
      stats = current;
      before = earlier;
      failed = false;
    } catch (e) {
      if (stats === null) failed = true;
      toast.error(errorMessage(e));
    }
    historyStorage().then((s) => { storage = s; }).catch(() => { storage = null; });
    modelsInstalled().then((m) => { models = m; }).catch(() => { models = null; });
  }

  onMount(() => {
    listMicrophones().then((m) => { mics = m; }).catch(() => { mics = []; });
    const onFocus = () => { load(); };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  });
  // Reloads when the period changes; the load itself must not count as something to react to.
  $effect(() => {
    const chosen = period;
    try { localStorage.setItem(PERIOD_KEY, chosen); } catch { /* The choice is not remembered. */ }
    untrack(() => { load(chosen); });
  });

  const num = (n: number) => n.toLocaleString(uiLocale());
  const secs = (ms: number) => t("home.secs", { n: (ms / 1000).toLocaleString(uiLocale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }) });
  const dateOf = (day: number) => dayDate(day).toLocaleDateString(uiLocale(), { month: "numeric", day: "numeric" });
  const share = (count: number, whole: number) => t("home.row.count_share", { n: num(count), p: String(percent(count, whole)) });

  /** A length of time as figures with units: hours and minutes, or minutes and seconds when short. */
  function span(ms: number): { value: string; unit?: string }[] {
    const p = durationParts(ms);
    if (p.hours > 0) return [{ value: num(p.hours), unit: t("home.unit.hours") }, { value: String(p.minutes), unit: t("home.unit.minutes") }];
    if (p.minutes > 0) return [{ value: String(p.minutes), unit: t("home.unit.minutes") }, { value: String(p.seconds), unit: t("home.unit.seconds") }];
    return [{ value: String(p.seconds), unit: t("home.unit.seconds") }];
  }
  const spanText = (ms: number) => span(ms).map((p) => `${p.value} ${p.unit}`).join(" ");

  const totals = $derived(stats?.totals ?? null);
  const empty = $derived(!!totals && totals.count === 0);
  const today = $derived(localDay(now));
  const typing = $derived(config.value?.general.typing_chars_per_min ?? 40);
  const firstDay = $derived.by(() => {
    if (!stats) return today;
    return period === "all" ? (stats.days[0]?.day ?? today) : localDay(new Date(periodStart(period, now) * 1000));
  });

  // The tiles: a figure, how it compares with the period before, and one column per day of the period.
  const perDay = $derived.by(() => {
    if (!stats || firstDay >= today) return null;
    const chars = fillDays(stats.days, firstDay, today);
    const count = fillDays(stats.days, firstDay, today, "count");
    const audio = fillDays(stats.days, firstDay, today, "audio_ms");
    const saved = chars.map((c, i) => Math.max(0, (c * 60_000) / Math.max(1, typing) - audio[i]));
    return { chars, count, audio, saved };
  });
  /** The change against the period before as "+12%", or null when there is nothing to compare with. */
  function compared(field: "chars" | "count" | "audio_ms"): string | null {
    if (!stats || !before || period === "all") return null;
    const earlier = before.totals[field] - stats.totals[field];
    const c = change(stats.totals[field], earlier);
    if (c === null) return null;
    return t(`home.compare.${period}`, { p: `${c > 0 ? "+" : ""}${c}%` });
  }
  const tiles = $derived.by(() => {
    if (!stats || !totals) return null;
    return {
      "t-chars": {
        parts: [{ value: num(totals.chars) }], label: t("home.tile.chars"),
        detail: totals.count ? t("home.tile.chars_detail", { n: num(Math.round(totals.chars / totals.count)) }) : undefined,
        compare: compared("chars"), spark: perDay?.chars,
      },
      "t-count": {
        parts: [{ value: num(totals.count) }], label: t("home.tile.count"),
        detail: periodDays.span > 1 && totals.count ? t("home.tile.count_detail", { n: num(Math.round(totals.count / periodDays.span)) }) : undefined,
        compare: compared("count"), spark: perDay?.count,
      },
      "t-audio": { parts: span(totals.audio_ms), label: t("home.tile.audio"), detail: t("home.tile.audio_detail"), compare: compared("audio_ms"), spark: perDay?.audio },
      "t-saved": {
        parts: stats.saved_ms === null ? [{ value: t("home.none") }] : span(stats.saved_ms), label: t("home.tile.saved"),
        detail: t("home.tile.saved_detail", { n: String(typing) }), compare: null, spark: perDay?.saved,
      },
    } as Record<string, { parts: { value: string; unit?: string }[]; label: string; detail?: string; compare: string | null; spark?: number[] }>;
  });

  // Days used: always over all time. The heatmap shows as many weeks as its card gives room for.
  let heatWidth = $state(0);
  const weeks = $derived(heatWeeks(heatWidth));
  const heat = $derived.by(() => {
    const rows = stats?.active_days ?? [];
    const from = today - weeks * 7 + 1;
    const chars = fillDays(rows, from, today);
    const max = Math.max(0, ...chars);
    return {
      levels: chars.map((c) => heatLevel(c, max)),
      titles: chars.map((c, i) => t("home.days.cell", { date: dateOf(from + i), n: num(c) })),
      months: monthLabels(from, weeks, (d) => d.toLocaleDateString(uiLocale(), { month: "short" })),
    };
  });
  const runs = $derived(streaks((stats?.active_days ?? []).map((d) => d.day), today));
  const periodDays = $derived.by(() => {
    if (!stats) return { used: 0, span: 0 };
    const first = period === "all" ? (stats.active_days[0]?.day ?? today) : localDay(new Date(periodStart(period, now) * 1000));
    return { used: stats.days.length, span: today - first + 1 };
  });

  // Characters per day, by week when the period is long.
  const daily = $derived.by(() => {
    if (!stats || stats.days.length === 0) return null;
    const first = firstDay;
    const values0 = fillDays(stats.days, first, today);
    const weekly = values0.length > 90;
    const values = weekly ? groupSums(values0, 7) : values0;
    const peak = stats.days.reduce((a, b) => (b.chars > a.chars ? b : a));
    return {
      values, weekly, peak, first, days: values0.length,
      titles: values.map((v, i) => t("home.days.cell", { date: dateOf(first + i * (weekly ? 7 : 1)), n: num(v) })),
    };
  });
  /** Date marks under the daily columns: more of them when the card is wider. */
  function dailyLabels(w: number): string[] {
    if (!daily) return [];
    if (daily.days <= 4) return [dateOf(daily.first), dateOf(today)];
    const n = w === 1 ? 3 : w === 2 ? 5 : 8;
    return Array.from({ length: n }, (_, i) => dateOf(daily.first + Math.round((daily.days - 1) * (i / (n - 1)))));
  }
  const hourLabels = (w: number) => (w === 1 ? ["0", "12", "23"] : w === 2 ? ["0", "6", "12", "18", "23"] : ["0", "3", "6", "9", "12", "15", "18", "21", "23"]);

  /** How many list rows a card of this size holds. */
  const listRoom = (w: number, h: number) => (h > 1 ? 10 : 4) * (w >= 2 ? 2 : 1);
  function appRows(w: number, h: number) {
    if (!stats || !totals) return [];
    const room = listRoom(w, h);
    const { top, others } = topWithOthers(stats.apps, stats.apps.length > room ? room - 1 : room);
    const rows = top.map((a) => ({ label: a.name || t("home.app_unknown"), value: a.count, note: share(a.count, totals.count), muted: !a.name }));
    if (others) rows.push({ label: t("home.apps.others", { n: String(others.names) }), value: others.count, note: share(others.count, totals.count), muted: true });
    return rows;
  }
  const profileRows = $derived.by(() => {
    if (!stats || !totals) return [];
    return stats.profiles.map((p) => {
      const known = config.value?.profiles.find((x) => x.id === p.name);
      return { label: known ? known.name : `${p.name}${t("history.deleted_profile")}`, value: p.count, note: share(p.count, totals.count), muted: !known };
    });
  });
  const scriptRows = $derived.by(() => {
    if (!totals) return [];
    const whole = totals.han + totals.latin + totals.kana;
    return ([["han", totals.han], ["latin", totals.latin], ["kana", totals.kana]] as const)
      .filter(([, n]) => n > 0)
      .map(([k, n]) => ({ label: t(`home.lang.${k}`), value: n, note: `${percent(n, whole)}%` }));
  });

  const cpm = $derived(totals ? speed(totals.speed_chars, totals.speed_audio_ms) : null);
  const modelBytes = $derived((models ?? []).reduce((n, m) => n + m.size_bytes, 0));
  /** The figures of the four list cards; each becomes a row at one column and a small cell when wider. */
  function figures(id: CardId): { label: string; value: string }[] | null {
    if (!totals) return null;
    switch (id) {
      case "speed":
        if (cpm === null) return null;
        return [
          { label: t("home.speed.average"), value: t("home.speed.cpm", { n: num(cpm) }) },
          { label: t("home.speed.fastest"), value: t("home.speed.cpm", { n: num(totals.max_cpm) }) },
          { label: t("home.speed.clip"), value: secs(totals.timed_count ? totals.audio_ms / totals.timed_count : 0) },
          { label: t("home.speed.longest_audio"), value: spanText(totals.max_audio_ms) },
          { label: t("home.speed.longest_chars"), value: t("home.chars", { n: num(totals.max_chars) }) },
        ];
      case "rewrite":
        if (empty) return null;
        return [
          { label: t("home.rewrite.changed"), value: share(totals.changed, totals.count) },
          { label: t("home.rewrite.fixes"), value: t("home.times", { n: num(totals.dict_fixes) }) },
          { label: t("home.rewrite.snippets"), value: t("home.times", { n: num(totals.snippets) }) },
          { label: t("home.rewrite.none"), value: t("home.items", { n: num(totals.unrewritten) }) },
          { label: t("home.rewrite.failed"), value: t("home.items", { n: num(totals.degraded) }) },
        ];
      case "time":
        if (empty) return null;
        return [
          { label: t("home.time.average"), value: secs((totals.asr_ms + totals.llm_ms) / totals.count) },
          { label: t("home.time.asr"), value: secs(totals.asr_ms / totals.count) },
          { label: t("home.time.llm"), value: totals.llm_count ? secs(totals.llm_ms / totals.llm_count) : t("home.none") },
          { label: t("home.time.slowest"), value: secs(totals.max_total_ms) },
        ];
      case "storage":
        return [
          { label: t("home.storage.rows"), value: storage ? t("home.items", { n: num(storage.rows) }) : t("home.none") },
          { label: t("home.storage.recordings"), value: storage ? t("home.storage.count_size", { n: num(storage.recordings), size: formatBytes(storage.recording_bytes) }) : t("home.none") },
          { label: t("home.storage.models"), value: models ? t("home.storage.count_size", { n: num(models.length), size: formatBytes(modelBytes) }) : t("home.none") },
        ];
      default:
        return null;
    }
  }
  const TITLE: Record<CardId, string> = {
    "t-chars": "home.tile.chars", "t-count": "home.tile.count", "t-audio": "home.tile.audio", "t-saved": "home.tile.saved",
    days: "home.days.title", daily: "home.daily.title", apps: "home.apps.title", profiles: "home.profiles.title", hours: "home.hours.title",
    speed: "home.speed.title", rewrite: "home.rewrite.title", time: "home.time.title", lang: "home.lang.title", storage: "home.storage.title",
  };

  // The default profile's flow in one line.
  const flowLine = $derived.by(() => {
    const p = defaultProfile(config.value);
    if (!p) return null;
    const providers = config.value?.providers ?? [];
    const installed = { ids: models ? new Set(models.map((m) => m.id)) : null, paths: new Set((models ?? []).map((m) => m.path)) };
    return [
      micLine(client.value?.microphone ?? "builtin", mics).name,
      engineLine(providers, p.asr_provider, installed).name,
      engineLine(providers, p.llm_provider, installed).name,
      outputLine(p, client.value?.insertion.default).name,
    ].join(t("home.flow_sep"));
  });

  // ---- The grid of cards: where each one sits, and the editing of that.
  let layout = $state<StatsCard[]>(sanitized(client.value?.stats_layout));
  let editing = $state(false);
  let gridEl = $state<HTMLElement | null>(null);
  /** The card whose size picker is open, and the size under the pointer in it. */
  let picker = $state<string | null>(null);
  let pickerHover = $state<{ w: number; h: number } | null>(null);
  type Hover =
    | { kind: "cell"; row: number; col: number; w: number; h: number; ok: boolean }
    | { kind: "row"; row: number; col: number; ok: boolean }
    | { kind: "swap"; id: string; ok: boolean };
  let hover = $state<Hover | null>(null);
  let dragging = $state<string | null>(null);
  type Fit = { w: number; h: number; row: number; col: number };
  type Drag =
    | { kind: "move"; id: string; ghost: HTMLElement; dx: number; dy: number }
    | { kind: "resize"; id: string; base: StatsCard[]; left: number; top: number; x0: number; y0: number; moved: boolean; target: HTMLElement | null; best: Fit | null };
  let drag: Drag | null = null;
  const shownCards = $derived(layout.filter((c) => !c.hidden));
  const hiddenCards = $derived(layout.filter((c) => c.hidden));
  const totalRows = $derived(rowCount(layout));

  // The settings may change behind the page (another window, a reset); the page follows unless it is being edited.
  $effect(() => {
    const stored = sanitized(client.value?.stats_layout);
    untrack(() => { if (!editing && !sameLayout(stored, layout)) layout = stored; });
  });

  const colWidth = () => (gridEl ? (gridEl.getBoundingClientRect().width - (COLS - 1) * GRID_GAP) / COLS : 0);
  const spanWidth = (w: number) => colWidth() * w + GRID_GAP * (w - 1);
  const spanHeight = (h: number) => ROW_HEIGHT * h + GRID_GAP * (h - 1);

  /** Applies a new arrangement: the cards glide from where they were, and the settings keep it; `still` is a card that follows the pointer instead. */
  async function apply(next: StatsCard[], save = true, still?: string) {
    if (sameLayout(next, layout)) return;
    const was = new Map<string, DOMRect>();
    gridEl?.querySelectorAll<HTMLElement>(".card").forEach((el) => was.set(el.dataset.id!, el.getBoundingClientRect()));
    layout = next;
    await tick();
    gridEl?.querySelectorAll<HTMLElement>(".card").forEach((el) => {
      const a = was.get(el.dataset.id!);
      if (!a || el.dataset.id === still) return;
      const b = el.getBoundingClientRect();
      const dx = a.left - b.left, dy = a.top - b.top;
      if (!dx && !dy && a.width === b.width && a.height === b.height) return;
      el.animate(
        [{ transform: `translate(${dx}px, ${dy}px)`, width: `${a.width}px`, height: `${a.height}px` }, { transform: "none", width: `${b.width}px`, height: `${b.height}px` }],
        { duration: 240, easing: "cubic-bezier(.2,.7,.2,1)" },
      );
    });
    if (save && client.value) client.save({ ...$state.snapshot(client.value), stats_layout: $state.snapshot(next) }).catch((e) => toast.error(errorMessage(e)));
  }

  function startMove(id: string, el: HTMLElement, e: PointerEvent) {
    const card = layout.find((c) => c.id === id)!;
    const r = el.getBoundingClientRect();
    const ghost = el.cloneNode(true) as HTMLElement;
    ghost.classList.add("ghost", "card");
    ghost.classList.remove("chip");
    ghost.querySelectorAll(".tools, .handle, .picker").forEach((x) => x.remove());
    const fromCard = el.classList.contains("card");
    // Inline, so the card's own scoped rules cannot override them.
    Object.assign(ghost.style, {
      position: "fixed", zIndex: "10", pointerEvents: "none", margin: "0",
      width: `${fromCard ? r.width : spanWidth(card.w)}px`, height: `${fromCard ? r.height : spanHeight(card.h)}px`,
      left: `${r.left}px`, top: `${r.top}px`, gridColumn: "auto", gridRow: "auto",
    });
    document.body.appendChild(ghost);
    drag = { kind: "move", id, ghost, dx: e.clientX - r.left, dy: e.clientY - r.top };
    dragging = id;
    hover = null;
    picker = null;
  }
  function onGridDown(e: PointerEvent) {
    if (!editing || e.button !== 0) return;
    const target = e.target as HTMLElement;
    const cardEl = target.closest<HTMLElement>(".card");
    if (!cardEl || target.closest(".tools") || target.closest(".picker")) return;
    const id = cardEl.dataset.id!;
    if (target.closest(".handle")) {
      const r = cardEl.getBoundingClientRect();
      drag = { kind: "resize", id, base: $state.snapshot(layout), left: r.left, top: r.top, x0: e.clientX, y0: e.clientY, moved: false, target: null, best: null };
    } else {
      startMove(id, cardEl, e);
    }
    gridEl?.setPointerCapture(e.pointerId);
  }
  function onTrayDown(e: PointerEvent) {
    const chip = (e.target as HTMLElement).closest<HTMLElement>(".chip");
    if (!chip || !editing) return;
    startMove(chip.dataset.id!, chip, e);
    gridEl?.setPointerCapture(e.pointerId);
  }
  async function onGridMove(e: PointerEvent) {
    if (!drag || !gridEl) return;
    const card = layout.find((c) => c.id === drag!.id)!;
    const def = cardDef(card.id)!;
    if (drag.kind === "resize") {
      if (Math.abs(e.clientX - drag.x0) + Math.abs(e.clientY - drag.y0) > 4) drag.moved = true;
      if (!drag.moved) return;
      const el = gridEl.querySelector<HTMLElement>(`.card[data-id="${card.id}"]`);
      if (!el) return;
      // The card's edges follow the pointer, measured from where the card is now (a preview may have
      // slid it left); the dashed frame shows where it will settle.
      const here = el.getBoundingClientRect();
      const liveW = Math.max(colWidth() * 0.7, Math.min(spanWidth(COLS) + 20, e.clientX - here.left + 12));
      const liveH = Math.max(ROW_HEIGHT * 0.7, Math.min(spanHeight(2) + 20, e.clientY - here.top + 12));
      el.classList.add("resizing");
      el.style.width = `${liveW}px`;
      el.style.height = `${liveH}px`;
      const wantW = Math.max(def.wmin, Math.min(def.wmax, Math.round((liveW + GRID_GAP) / (colWidth() + GRID_GAP))));
      const wantH = Math.max(def.hmin, Math.min(def.hmax, Math.round((liveH + GRID_GAP) / (ROW_HEIGHT + GRID_GAP))));
      // The largest size toward what is wanted that can be had, always from the arrangement the drag
      // started with, so shrinking again lets pushed neighbours come back.
      const base = drag.base, me0 = base.find((c) => c.id === card.id)!;
      let best: Fit | null = null, preview: StatsCard[] = base;
      for (let w = wantW; w >= def.wmin && !best; w--) for (let h = wantH; h >= def.hmin && !best; h--) {
        const next = resized(base, card.id, w, h);
        if (next !== base || (w === me0.w && h === me0.h)) { const me = next.find((c) => c.id === card.id)!; best = { w, h, row: me.row, col: me.col }; preview = next; }
      }
      const changed = JSON.stringify(best) !== JSON.stringify(drag.best);
      drag.best = best;
      // The neighbours move out of the way while the pointer is still down; when the card itself slid
      // left, its edge is set again from the new place so it stays under the pointer.
      if (changed) {
        await apply(preview, false, card.id);
        const now = el.getBoundingClientRect();
        el.classList.add("resizing");
        el.style.width = `${Math.max(colWidth() * 0.7, Math.min(spanWidth(COLS) + 20, e.clientX - now.left + 12))}px`;
        el.style.height = `${Math.max(ROW_HEIGHT * 0.7, Math.min(spanHeight(2) + 20, e.clientY - now.top + 12))}px`;
      }
      if (!drag.target) {
        drag.target = document.createElement("div");
        drag.target.className = "target";
        gridEl.appendChild(drag.target);
      }
      if (best) {
        Object.assign(drag.target.style, {
          left: `${best.col * (colWidth() + GRID_GAP)}px`, top: `${best.row * (ROW_HEIGHT + GRID_GAP)}px`,
          width: `${spanWidth(best.w)}px`, height: `${spanHeight(best.h)}px`,
        });
      }
      return;
    }
    drag.ghost.style.left = `${e.clientX - drag.dx}px`;
    drag.ghost.style.top = `${e.clientY - drag.dy}px`;
    const els = document.elementsFromPoint(e.clientX, e.clientY) as HTMLElement[];
    const zone = els.find((x) => x.classList?.contains("rowzone"));
    const cell = els.find((x) => x.classList?.contains("cell"));
    const over = els.find((x) => x.classList?.contains("card") && !x.classList.contains("ghost") && x.dataset.id !== card.id);
    const gl = gridEl.getBoundingClientRect();
    const col = Math.max(0, Math.min(COLS - card.w, Math.floor((e.clientX - gl.left) / (colWidth() + GRID_GAP))));
    let next: Hover | null = null;
    if (zone) {
      const row = Number(zone.dataset.row);
      next = { kind: "row", row, col, ok: canInsertRow(layout, row, card.id) };
    } else if (cell) {
      const row = Number(cell.dataset.row), c = Math.min(Number(cell.dataset.col), COLS - card.w);
      next = { kind: "cell", row, col: c, w: card.w, h: card.h, ok: fits(layout, row, c, card.w, card.h, card.id) };
    } else if (over) {
      const o = layout.find((x) => x.id === over.dataset.id)!;
      next = { kind: "swap", id: o.id, ok: o.w === card.w && o.h === card.h };
    }
    if (JSON.stringify(next) !== JSON.stringify(hover)) hover = next;
  }
  function onGridUp() {
    if (!drag) return;
    const card = layout.find((c) => c.id === drag!.id)!;
    if (drag.kind === "resize") {
      const el = gridEl?.querySelector<HTMLElement>(`.card[data-id="${card.id}"]`);
      if (!drag.moved) {
        picker = picker === card.id ? null : card.id;
      } else {
        drag.target?.remove();
        const was = el?.getBoundingClientRect();
        if (el) { el.classList.remove("resizing"); el.style.width = ""; el.style.height = ""; }
        // The neighbours already moved during the drag; the card itself settles from its stretched size.
        if (el && was) {
          const b = el.getBoundingClientRect();
          el.animate([{ width: `${was.width}px`, height: `${was.height}px` }, { width: `${b.width}px`, height: `${b.height}px` }], { duration: 200, easing: "ease-out" });
        }
        if (!sameLayout(layout, drag.base) && client.value) client.save({ ...$state.snapshot(client.value), stats_layout: $state.snapshot(layout) }).catch((e) => toast.error(errorMessage(e)));
      }
      drag = null;
      return;
    }
    const target = hover;
    drag.ghost.remove();
    drag = null;
    dragging = null;
    hover = null;
    if (!target || !target.ok) return;
    if (target.kind === "row") apply(insertedAt(layout, card.id, target.row, target.col));
    else if (target.kind === "cell") apply(movedTo(layout, card.id, target.row, target.col));
    else apply(swapped(layout, card.id, target.id));
  }
  function pick(id: string, w: number, h: number) {
    picker = null;
    pickerHover = null;
    apply(resized(layout, id, w, h));
  }
  function hide(id: string) {
    picker = null;
    apply(hideCard(layout, id));
  }
  function sizeState(card: StatsCard, w: number, h: number): "current" | "pick" | "blocked" | "unsupported" {
    const def = cardDef(card.id)!;
    if (w === card.w && h === card.h) return "current";
    if (!supports(def, w, h)) return "unsupported";
    return canResize(layout, card.id, w, h) ? "pick" : "blocked";
  }
  const resizable = (id: string) => { const d = cardDef(id)!; return d.wmin !== d.wmax || d.hmin !== d.hmax; };
  function hoverClass(card: StatsCard): string {
    if (!hover || hover.kind !== "swap" || hover.id !== card.id) return "";
    return hover.ok ? "ok" : "bad";
  }
  function cellClass(row: number, col: number): string {
    if (!hover || hover.kind !== "cell") return "";
    if (row < hover.row || row >= hover.row + hover.h || col < hover.col || col >= hover.col + hover.w) return "";
    return hover.ok ? "ok" : "bad";
  }
</script>

<PageHead title={t("nav.home")} description={t("home.description")}>
  {#snippet actions()}
    <Segmented options={PERIODS.map((p) => ({ value: p, label: t(`home.period.${p}`) }))} bind:value={period} label={t("home.period.label")} />
    {#if editing}
      <button type="button" class="btn quiet" onclick={() => apply(DEFAULT_LAYOUT)}>{t("home.customize_reset")}</button>
      <button type="button" class="btn primary" onclick={() => { editing = false; picker = null; }}>{t("home.customize_done")}</button>
    {:else}
      <button type="button" class="btn" onclick={() => { editing = true; }} disabled={!stats}>{t("home.customize")}</button>
    {/if}
  {/snippet}
</PageHead>

{#snippet figureList(id: CardId, w: number)}
  {@const rows = figures(id)}
  {#if !rows}
    <p class="none">{t(id === "speed" ? "home.empty.speed" : "home.empty.block")}</p>
  {:else if w === 1}
    <dl class="kv">{#each rows as r (r.label)}<dt title={r.label}>{r.label}</dt><dd title={r.value}>{r.value}</dd>{/each}</dl>
  {:else}
    <div class="cells" style:--n={w === 2 ? 3 : 5}>{#each rows as r (r.label)}<div title="{r.label}: {r.value}"><b class="num">{r.value}</b><span>{r.label}</span></div>{/each}</div>
  {/if}
{/snippet}

{#snippet listCard(rows: { label: string; value: number; note: string; muted?: boolean }[], w: number, h: number)}
  {@const room = listRoom(w, h)}
  {#if rows.length}
    <BarList rows={rows.slice(0, room)} columns={w >= 2 ? 2 : 1} />
    {#if rows.length > room}<p class="more">{t("home.list.more", { n: String(rows.length - room) })}</p>{/if}
  {:else}
    <p class="none">{t("home.empty.block")}</p>
  {/if}
{/snippet}

{#snippet body(card: StatsCard)}
  {@const id = card.id as CardId}
  {#if tiles && id in tiles}
    {@const tile = tiles[id]}
    <div class="tile">
      <div class="label">{tile.label}</div>
      <div class="value num">{#each tile.parts as p, i (i)}<span>{p.value}</span>{#if p.unit}<small>{p.unit}</small>{/if}{/each}</div>
      {#if tile.detail}<div class="detail" title={tile.detail}>{tile.detail}</div>{/if}
      {#if tile.compare}<div class="compare" class:down={tile.compare.includes("-")}>{tile.compare}</div>{/if}
      {#if tile.spark && tile.spark.length > 1}
        <div class="spark"><Columns values={tile.spark} /></div>
        <div class="detail">{t("home.tile.spark")}</div>
      {/if}
    </div>
  {:else if id === "days"}
    <h2>{t("home.days.title")}<em>{t("home.days.range", { n: String(weeks) })}</em></h2>
    {#if card.w >= 2}<p class="sub" title={t("home.days.sub")}>{t("home.days.sub")}</p>{/if}
    <div class="heatwrap">
      <div class="heatroom" bind:clientWidth={heatWidth}><Heatmap levels={heat.levels} titles={heat.titles} months={heat.months} /></div>
      {#if card.w >= 2}
        <div class="facts">
          <div><b class="num">{t("home.days.n", { n: String(runs.current) })}</b><span>{t("home.days.current")}</span></div>
          <div><b class="num">{t("home.days.n", { n: String(runs.longest) })}</b><span>{t("home.days.longest")}</span></div>
          <div><b class="num">{periodDays.used} / {periodDays.span}</b><span>{t("home.days.active")}</span></div>
        </div>
      {/if}
    </div>
  {:else if id === "daily"}
    <h2>{t(daily?.weekly ? "home.daily.title_weekly" : "home.daily.title")}{#if daily && card.w >= 2}<em>{t("home.daily.peak", { n: num(daily.peak.chars), date: dateOf(daily.peak.day) })}</em>{/if}</h2>
    <p class="sub" title={t(daily?.weekly ? "home.daily.sub_weekly" : "home.daily.sub")}>{t(daily?.weekly ? "home.daily.sub_weekly" : "home.daily.sub")}</p>
    {#if daily}<div class="chart"><Columns values={daily.values} labels={dailyLabels(card.w)} titles={daily.titles} /></div>{:else}<p class="none">{t("home.empty.block")}</p>{/if}
  {:else if id === "apps"}
    <h2>{t("home.apps.title")}{#if stats?.apps.length}<em>{t("home.apps.total", { n: String(stats.apps.length) })}</em>{/if}</h2>
    <p class="sub" title={t("home.apps.sub")}>{t("home.apps.sub")}</p>
    {@render listCard(appRows(card.w, card.h), card.w, card.h)}
  {:else if id === "profiles"}
    <h2>{t("home.profiles.title")}</h2>
    <p class="sub" title={t("home.profiles.sub")}>{t("home.profiles.sub")}</p>
    {@render listCard(profileRows, card.w, card.h)}
  {:else if id === "hours"}
    <h2>{t("home.hours.title")}</h2>
    <p class="sub" title={t("home.hours.sub")}>{t("home.hours.sub")}</p>
    {#if empty || !stats}<p class="none">{t("home.empty.block")}</p>{:else}
      <div class="chart"><Columns values={stats.hours} labels={hourLabels(card.w)} titles={stats.hours.map((n, h) => t("home.hours.cell", { h: String(h), n: num(n) }))} /></div>
    {/if}
  {:else if id === "lang"}
    <h2>{t("home.lang.title")}</h2>
    <p class="sub" title={t("home.lang.sub")}>{t("home.lang.sub")}</p>
    {@render listCard(scriptRows, card.w, card.h)}
  {:else}
    <h2>{t(TITLE[id])}</h2>
    <p class="sub" title={t(`home.${id}.sub`)}>{t(`home.${id}.sub`)}</p>
    {@render figureList(id, card.w)}
  {/if}
{/snippet}

{#if failed}
  <div class="card plain">
    <Empty text={t("home.load_failed")}>
      {#snippet action()}<button type="button" class="btn" onclick={() => load()}>{t("history.retry")}</button>{/snippet}
    </Empty>
  </div>
{:else if totals && stats}
  {#if empty}<p class="hint">{t("home.empty.intro", { key: firstShortcut(client.value?.hotkey.shortcuts) })}</p>{/if}

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="grid" class:editing bind:this={gridEl} onpointerdown={onGridDown} onpointermove={onGridMove} onpointerup={onGridUp} onpointercancel={onGridUp}>
    {#each shownCards as card (card.id)}
      <section
        class="card {hoverClass(card)}"
        class:dragging={dragging === card.id}
        class:resizable={resizable(card.id)}
        data-id={card.id}
        style:grid-row="{card.row + 1} / span {card.h}"
        style:grid-column="{card.col + 1} / span {card.w}"
      >
        {@render body(card)}
        {#if editing}
          <div class="tools"><button type="button" class="tool" title={t("home.card.hide")} aria-label={t("home.card.hide")} onclick={() => hide(card.id)}>×</button></div>
          {#if resizable(card.id)}
            <div class="handle" title={t("home.card.handle")}><span class="grip"><i></i><i></i><i></i><i></i><i></i><i></i></span></div>
          {/if}
          {#if picker === card.id}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="picker" role="group" onmouseleave={() => { pickerHover = null; }}>
              {#each [1, 2] as h (h)}{#each [1, 2, 3] as w (w)}
                {@const state = sizeState(card, w, h)}
                {@const shown = pickerHover ?? { w: card.w, h: card.h }}
                <button
                  type="button"
                  class="size {state}"
                  class:on={w <= shown.w && h <= shown.h}
                  class:preview={pickerHover !== null}
                  title={t(`home.size.${state}`, { w: String(w), h: String(h) })}
                  aria-label={t(`home.size.${state}`, { w: String(w), h: String(h) })}
                  disabled={state === "blocked" || state === "unsupported"}
                  onmouseenter={() => { pickerHover = state === "blocked" || state === "unsupported" ? null : { w, h }; }}
                  onclick={() => pick(card.id, w, h)}
                ></button>
              {/each}{/each}
              <span class="cap num">{(pickerHover ?? card).w} × {(pickerHover ?? card).h}</span>
            </div>
          {/if}
        {/if}
      </section>
    {/each}
    {#if editing}
      {#each Array.from({ length: totalRows + 1 }, (_, r) => r) as row (row)}
        {#each [0, 1, 2] as col (col)}
          {#if !occupant(layout, row, col)}
            <div class="cell {cellClass(row, col)}" data-row={row} data-col={col} style:grid-row={row + 1} style:grid-column={col + 1}>{cellClass(row, col) ? "" : t("home.empty_cell")}</div>
          {/if}
        {/each}
        <div
          class="rowzone"
          class:ok={hover?.kind === "row" && hover.row === row && hover.ok}
          class:bad={hover?.kind === "row" && hover.row === row && !hover.ok}
          data-row={row}
          data-label={t("home.insert_row")}
          style:top="{row * (ROW_HEIGHT + GRID_GAP) - GRID_GAP}px"
        ></div>
      {/each}
    {/if}
  </div>

  {#if editing}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="tray" onpointerdown={onTrayDown}>
      {#if hiddenCards.length}
        <span>{t("home.hidden_cards")}</span>
        {#each hiddenCards as card (card.id)}<span class="chip" data-id={card.id}>{t(TITLE[card.id as CardId])}</span>{/each}
      {:else}
        <span>{t("home.hidden_none")}</span>
      {/if}
    </div>
  {/if}

  {#if flowLine}
    <div class="status">
      <Ring tone="accent" />
      <span class="grow">{t("home.flow_line", { flow: flowLine })}</span>
      <button type="button" class="btn quiet" onclick={() => guard.request(() => go("flow"))}>{t("home.flow_button")}</button>
    </div>
  {/if}
{/if}

<style>
  .grow { flex: 1; min-width: 0; }
  .hint { margin: 0 4px 18px; color: var(--ink-2); }

  /* Three columns, rows of one height; a card names its own row, column and span. */
  .grid { position: relative; display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); grid-auto-rows: 240px; gap: 14px; }
  .card { position: relative; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); padding: 22px 24px; }
  .card.plain { display: block; }
  h2 { margin: 0 0 4px; font-size: var(--text-lg); font-weight: 600; display: flex; align-items: baseline; gap: 12px; }
  h2 em { font-style: normal; font-weight: 400; font-size: var(--text-sm); color: var(--ink-2); margin-left: auto; text-align: right; white-space: nowrap; }
  /* One line: the cards have one height, so a long sentence is cut rather than pushing the content out. */
  .sub { margin: 0 0 12px; color: var(--ink-2); font-size: var(--text-sm); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .none { margin: 0; color: var(--ink-3); font-size: var(--text-control); }
  .more { margin: auto 0 0; padding-top: 8px; color: var(--ink-3); font-size: var(--text-xs); }

  /* Tiles */
  .tile { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .tile .label { color: var(--ink-2); font-size: var(--text-sm); }
  .tile .value { font: 700 var(--text-stat)/1.1 var(--font-round); white-space: nowrap; margin: 6px 0 6px; }
  .tile small { font: 600 var(--text)/1 var(--font); color: var(--ink-2); margin: 0 6px 0 4px; }
  .tile .detail { color: var(--ink-3); font-size: var(--text-xs); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tile .compare { margin-top: 6px; font-size: var(--text-sm); color: var(--ok, #198754); font-weight: 600; }
  .tile .compare.down { color: var(--ink-3); font-weight: 400; }
  .tile .spark { display: flex; flex-direction: column; flex: 1; min-height: 28px; margin-top: auto; padding-top: 10px; }

  /* Days used */
  /* Seven rows of squares plus the month row need the whole body, so this card's head is tighter. */
  .card[data-id="days"] h2 { margin-bottom: 2px; }
  .card[data-id="days"] .sub { margin-bottom: 6px; }
  .heatwrap { display: flex; gap: 32px; align-items: center; flex: 1; min-height: 0; }
  .heatroom { flex: 1; min-width: 0; display: flex; }
  /* A little air on the right, so the widest label does not sit on the padding edge. */
  .facts { display: flex; flex-direction: column; gap: 6px; min-width: 0; padding-right: 8px; }
  .facts b { display: block; font: 700 20px/1.1 var(--font-round); }
  .facts span { display: block; color: var(--ink-2); font-size: var(--text-xs); line-height: 1.3; }
  /* Room for the resize handle at the corner while editing. */
  .grid.editing .facts { padding-right: 20px; }

  .chart { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .kv { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 7px 14px; margin: 0; font-size: var(--text-sm); line-height: 1.4; align-content: start; }
  .kv dt { color: var(--ink-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .kv dd { margin: 0; font-weight: 600; text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .cells { display: grid; grid-template-columns: repeat(var(--n), minmax(0, 1fr)); gap: 8px; align-content: start; }
  .cells div { background: var(--sunken); border-radius: var(--radius); padding: 9px 12px; min-width: 0; }
  .cells b { display: block; font: 700 17px/1.2 var(--font-round); margin-bottom: 1px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .cells span { display: block; color: var(--ink-2); font-size: var(--text-xs); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  /* Editing */
  .grid.editing .card { cursor: grab; outline: 1.5px dashed color-mix(in srgb, var(--accent) 50%, transparent); outline-offset: -1.5px; }
  .grid.editing .card.resizing { z-index: 6; box-shadow: 0 12px 40px rgba(0, 0, 0, .25); }
  .card.dragging { opacity: .3; }
  .card.ok { outline: 2.5px solid var(--accent); outline-offset: -2.5px; box-shadow: 0 0 0 4px color-mix(in srgb, var(--accent) 25%, transparent); }
  .card.bad { outline: 2px solid var(--live); outline-offset: -2px; }
  .cell { border: 1.5px dashed var(--line); border-radius: var(--radius-group); display: flex; align-items: center; justify-content: center; color: var(--ink-3); font-size: var(--text-xs); }
  .cell.ok { border-style: solid; border-color: var(--accent); background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .cell.bad { border-color: var(--live); }
  /* A bare grey cross in the top-right corner, the grip's grey and size; the whole corner square is the button. */
  .tools { position: absolute; top: 0; right: 0; display: flex; z-index: 2; }
  .tool { width: 44px; height: 44px; padding: 0; border: 0; background: none; color: var(--ink-3); font: 300 26px/1 var(--font); display: flex; align-items: flex-start; justify-content: flex-end; padding: 9px 9px 0 0; box-sizing: border-box; cursor: pointer; }
  .tool:hover { color: var(--ink-2); }
  /* A short rounded bracket inside the bottom-right corner, as on a home-screen widget: drag it to resize. */
  /* The whole corner square resizes, so a press near the corner never moves the card by mistake; six dots mark it. */
  .handle { position: absolute; right: 0; bottom: 0; width: 44px; height: 44px; cursor: nwse-resize; z-index: 3; }
  .grip { position: absolute; right: 9px; bottom: 9px; width: 18px; height: 18px; }
  .grip i { position: absolute; width: 4px; height: 4px; border-radius: 2px; background: var(--ink-3); }
  .handle:hover .grip i { background: var(--ink-2); }
  .grip i:nth-child(1) { right: 0; bottom: 0; }
  .grip i:nth-child(2) { right: 7px; bottom: 0; }
  .grip i:nth-child(3) { right: 0; bottom: 7px; }
  .grip i:nth-child(4) { right: 14px; bottom: 0; }
  .grip i:nth-child(5) { right: 7px; bottom: 7px; }
  .grip i:nth-child(6) { right: 0; bottom: 14px; }
  .picker { position: absolute; right: 8px; bottom: 36px; z-index: 5; display: grid; grid-template-columns: repeat(3, 30px); gap: 5px; padding: 10px; background: var(--surface); border: 1px solid var(--line); border-radius: 12px; box-shadow: 0 12px 40px rgba(0, 0, 0, .25); }
  .size { display: block; height: 22px; border-radius: 6px; border: 0; background: var(--ink-3); opacity: .9; cursor: pointer; position: relative; padding: 0; }
  .size:hover { opacity: 1; }
  .size.on { background: var(--accent); opacity: 1; }
  .size.on.preview { background: color-mix(in srgb, var(--accent) 80%, var(--surface)); }
  .size.blocked { opacity: .35; cursor: default; }
  .size.blocked::after { content: ""; position: absolute; left: 50%; top: -3px; bottom: -3px; width: 1.5px; margin-left: -.75px; background: var(--surface); transform: rotate(45deg); }
  .size.unsupported { background: none; border: 1.5px solid var(--ink-3); opacity: .4; cursor: default; }
  .picker .cap { grid-column: 1 / -1; text-align: center; font-size: var(--text-xs); color: var(--ink-3); }
  .rowzone { position: absolute; left: 0; right: 0; height: 14px; z-index: 4; }
  .rowzone::before { content: ""; position: absolute; left: 0; right: 0; top: 50%; height: 3px; margin-top: -1.5px; border-radius: 2px; }
  .rowzone.ok::before { background: var(--accent); }
  .rowzone.ok::after { content: attr(data-label); position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%); background: var(--accent); color: var(--accent-ink, #fff); font-size: var(--text-xs); padding: 2px 8px; border-radius: 6px; white-space: nowrap; }
  .rowzone.bad::before { background: var(--live); }
  :global(.grid .target) { position: absolute; border: 2px dashed var(--accent); border-radius: var(--radius-group); background: color-mix(in srgb, var(--accent) 8%, transparent); pointer-events: none; z-index: 1; transition: left .12s, top .12s, width .12s, height .12s; }
  :global(body > .ghost) { position: fixed; pointer-events: none; z-index: 10; opacity: .92; box-shadow: 0 12px 40px rgba(0, 0, 0, .3); margin: 0; }
  .tray { margin-top: 14px; padding: 14px 16px; border: 1.5px dashed var(--line); border-radius: var(--radius-group); display: flex; gap: 10px; align-items: center; flex-wrap: wrap; font-size: var(--text-sm); color: var(--ink-2); }
  .chip { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius); padding: 6px 10px; cursor: grab; color: var(--ink); }

  .status { display: flex; align-items: center; gap: 12px; margin-top: 14px; padding: 14px var(--row-x); border-radius: var(--radius-group); background: var(--surface); border: 1px solid var(--line); font-size: var(--text-control); color: var(--ink-2); }
</style>
