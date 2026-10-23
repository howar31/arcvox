<script lang="ts">
  import { save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { untrack } from "svelte";
  import { errorMessage, getStats, shareCopyImage, shareSaveImage, type Stats } from "../../../lib/api";
  import { uiLocale } from "../../../lib/format";
  import { t } from "../../../lib/i18n";
  import {
    CARD_WIDTH, cardFigures, cardHeight, cardSvg, CHARTS, DEFAULT_OPTIONS, EXTRA_KEYS, fileName, MAIN_KEYS, MAX_EXTRA, normaliseOptions, PERIODS, SIZES, STYLES, svgDataUrl,
    type FigureKey, type ShareOptions, type SharePeriod, type ShareStrings,
  } from "../../../lib/shareCard";
  import { localDay, periodStart, type Period } from "../../../lib/statsView";
  import Modal from "../../../lib/ui/Modal.svelte";
  import Segmented from "../../../lib/ui/Segmented.svelte";
  import { toast } from "../../../lib/ui/toast.svelte";

  /**
   * The share card: a dialog with the card as a live preview on the left and its choices on the right.
   * It reads the statistics of its own period, and the choices are kept in localStorage.
   */
  let { open = $bindable(false), pagePeriod }: { open?: boolean; pagePeriod: Period } = $props();

  const KEY = "arcvox.share.options";
  /** The preview's largest width and height; a tall size is shown narrower. */
  const PREVIEW_WIDTH = 400;
  const PREVIEW_HEIGHT = 520;

  let stats = $state<Stats | null>(null);
  let options = $state<ShareOptions>(DEFAULT_OPTIONS);
  let busy = $state(false);
  let now = $state(new Date());

  const strings = $derived.by<ShareStrings>(() => ({
    period: { week: t("share.period.week"), month: t("share.period.month"), year: t("share.period.year"), all: t("share.period.all") },
    caption: Object.fromEntries(["chars", "saved", "count", "days", "streak", "cpm", "peak", "audio"].map((k) => [k, t(`share.caption.${k}`)])) as Record<FigureKey, string>,
    unit: { chars: t("share.unit.chars"), count: t("share.unit.count"), days: t("share.unit.days"), cpm: t("share.unit.cpm"), hours: t("share.unit.hours"), minutes: t("share.unit.minutes") },
    unavailable: { saved: t("share.unavailable.saved"), cpm: t("share.unavailable.cpm") },
    chart: { days: t("share.chart_caption.days"), daily: t("share.chart_caption.daily"), weekly: t("share.chart_caption.weekly") },
  }));
  const today = $derived(localDay(now));
  const figures = $derived(stats ? cardFigures(stats, today, uiLocale(), strings) : null);
  const svg = $derived(stats ? cardSvg(stats, options, strings, today, uiLocale(), now) : null);
  const previewScale = $derived(Math.min(PREVIEW_WIDTH / CARD_WIDTH, PREVIEW_HEIGHT / cardHeight(options.size)));
  const previewWidth = $derived(Math.round(CARD_WIDTH * previewScale));
  const previewHeight = $derived(Math.round(cardHeight(options.size) * previewScale));

  function stored(): Partial<ShareOptions> | null {
    try { return JSON.parse(localStorage.getItem(KEY) ?? "null"); } catch { return null; }
  }
  function remember() {
    try { localStorage.setItem(KEY, JSON.stringify(options)); } catch { /* The choices are not remembered. */ }
  }

  async function load(period: SharePeriod) {
    now = new Date();
    try {
      stats = await getStats(periodStart(period, now), -now.getTimezoneOffset() * 60);
    } catch (e) {
      toast.error(errorMessage(e));
      open = false;
    }
  }

  // Opening reads the stored choices, starts on the page's period when the card has it, and loads.
  $effect(() => {
    if (!open) return;
    untrack(() => {
      const base = stored() ?? {};
      const period = PERIODS.includes(pagePeriod as SharePeriod) ? (pagePeriod as SharePeriod) : (base.period ?? DEFAULT_OPTIONS.period);
      options = { ...DEFAULT_OPTIONS, ...base, period } as ShareOptions;
      stats = null;
      load(period);
    });
  });
  // The figures decide which choices stand; a period change reloads.
  $effect(() => {
    if (!figures) return;
    const next = normaliseOptions(untrack(() => options), figures);
    if (JSON.stringify(next) !== JSON.stringify(untrack(() => options))) options = next;
  });

  function set<K extends keyof ShareOptions>(key: K, value: ShareOptions[K]) {
    options = { ...options, [key]: value };
    remember();
    if (key === "period") load(value as SharePeriod);
  }
  function toggleExtra(key: FigureKey) {
    const on = options.extra.includes(key);
    if (!on && options.extra.length >= MAX_EXTRA) return;
    set("extra", on ? options.extra.filter((k) => k !== key) : [...options.extra, key]);
  }
  const extraBlocked = (key: FigureKey) => !figures?.[key]?.available || key === options.main || (!options.extra.includes(key) && options.extra.length >= MAX_EXTRA);

  /** The card as PNG bytes at 2x. */
  async function render(): Promise<Uint8Array> {
    if (!svg) throw new Error("no card");
    const img = new Image();
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve();
      img.onerror = () => reject(new Error("svg"));
      img.src = svgDataUrl(svg);
    });
    const canvas = document.createElement("canvas");
    canvas.width = CARD_WIDTH * 2;
    canvas.height = cardHeight(options.size) * 2;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("canvas");
    ctx.scale(2, 2);
    ctx.drawImage(img, 0, 0);
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
    if (!blob) throw new Error("png");
    return new Uint8Array(await blob.arrayBuffer());
  }

  async function copy() {
    busy = true;
    try {
      await shareCopyImage(await render());
      toast.ok(t("share.copied"));
      open = false;
    } catch (e) {
      toast.error(e instanceof Error ? t("share.failed") : errorMessage(e));
    }
    busy = false;
  }

  async function save() {
    const name = fileName(t("share.file_word"), new Date());
    const path = await saveDialog({ defaultPath: name, filters: [{ name: "PNG", extensions: ["png"] }] });
    if (!path) return;
    busy = true;
    try {
      const written = await shareSaveImage(path, await render());
      toast.ok(t("share.saved", { name: written.split("/").pop() ?? written }));
      open = false;
    } catch (e) {
      toast.error(e instanceof Error ? t("share.failed") : errorMessage(e));
    }
    busy = false;
  }
</script>

<Modal bind:open title={t("share.title")} size="lg">
  <div class="share">
    <div class="preview" style:width="{previewWidth}px" style:height="{previewHeight}px">
      {#if svg}
        <img src={svgDataUrl(svg)} alt="" width={previewWidth} height={previewHeight} />
      {:else}
        <p class="loading">{t("share.loading")}</p>
      {/if}
    </div>
    <div class="opts">
      <div class="opt">
        <div class="lab">{t("share.period")}</div>
        <Segmented options={PERIODS.map((p) => ({ value: p, label: t(`share.pick.${p}`) }))} bind:value={() => options.period, (v) => set("period", v)} label={t("share.period")} />
      </div>
      <div class="opt">
        <div class="lab">{t("share.main")}</div>
        <div class="chips" role="radiogroup" aria-label={t("share.main")}>
          {#each MAIN_KEYS as k (k)}
            <button type="button" class="chip" class:on={options.main === k} role="radio" aria-checked={options.main === k} disabled={!figures?.[k]?.available} title={figures && !figures[k].available ? figures[k].caption : undefined} onclick={() => set("main", k)}>{t(`share.fig.${k}`)}</button>
          {/each}
        </div>
      </div>
      <div class="opt">
        <div class="lab">{t("share.extra", { n: String(MAX_EXTRA) })}</div>
        <div class="chips">
          {#each EXTRA_KEYS as k (k)}
            <button type="button" class="chip" class:on={options.extra.includes(k)} role="checkbox" aria-checked={options.extra.includes(k)} disabled={extraBlocked(k)} title={figures && !figures[k].available ? figures[k].caption : undefined} onclick={() => toggleExtra(k)}>{t(`share.fig.${k}`)}</button>
          {/each}
        </div>
      </div>
      <div class="opt">
        <div class="lab">{t("share.chart")}</div>
        <Segmented options={CHARTS.map((c) => ({ value: c, label: t(`share.chart.${c}`) }))} bind:value={() => options.chart, (v) => set("chart", v)} label={t("share.chart")} />
      </div>
      <div class="opt">
        <div class="lab">{t("share.style")}</div>
        <div class="swatches" role="radiogroup" aria-label={t("share.style")}>
          {#each STYLES as s (s)}
            <button type="button" class="sw {s}" class:on={options.style === s} role="radio" aria-checked={options.style === s} onclick={() => set("style", s)}><span>{t(`share.style.${s}`)}</span></button>
          {/each}
        </div>
      </div>
      <div class="opt">
        <div class="lab">{t("share.size")}</div>
        <Segmented options={SIZES.map((s) => ({ value: s, label: t(`share.size.${s}`) }))} bind:value={() => options.size, (v) => set("size", v)} label={t("share.size")} />
      </div>
      <p class="note">{t("share.note")}</p>
    </div>
  </div>
  {#snippet actions()}
    <button type="button" class="btn quiet" onclick={() => (open = false)}>{t("common.cancel")}</button>
    <button type="button" class="btn" disabled={!svg || busy} onclick={save}>{t("share.save")}</button>
    <button type="button" class="btn primary" disabled={!svg || busy} onclick={copy}>{t("share.copy")}</button>
  {/snippet}
</Modal>

<style>
  .share { display: flex; gap: 24px; align-items: flex-start; }
  .preview { flex: none; border-radius: 14px; overflow: hidden; background: var(--sunken); box-shadow: var(--shadow); display: grid; place-items: center; }
  .preview img { display: block; }
  .loading { margin: 0; color: var(--ink-3); font-size: var(--text-sm); }
  .opts { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 14px; }
  .lab { font-size: var(--text-sm); color: var(--ink-2); margin-bottom: 6px; }
  .chips { display: flex; flex-wrap: wrap; gap: 8px; }
  .chip { height: 30px; padding: 0 12px; border-radius: 15px; background: var(--surface); border: 1px solid var(--line-strong); font: inherit; font-size: var(--text-sm); color: var(--ink-2); cursor: default; }
  .chip.on { background: var(--accent-soft); border-color: var(--accent); color: var(--accent); font-weight: 600; }
  .chip:disabled:not(.on) { opacity: .45; }
  .swatches { display: flex; gap: 10px; }
  .sw { position: relative; width: 60px; height: 60px; margin-bottom: 20px; padding: 0; border-radius: 12px; border: 2px solid transparent; cursor: default; }
  .sw.on { border-color: var(--accent); }
  .sw.dark { background: #0F1220; }
  .sw.light { background: #FFFFFF; box-shadow: inset 0 0 0 1px var(--line-strong); }
  .sw.brand { background: linear-gradient(145deg, #FF7AB8, #2747D6 58%, #7FC4FF); }
  .sw span { position: absolute; left: -10px; right: -10px; bottom: -22px; text-align: center; font-size: var(--text-xs); color: var(--ink-2); }
  .note { margin: 4px 0 0; color: var(--ink-3); font-size: var(--text-xs); }
</style>
