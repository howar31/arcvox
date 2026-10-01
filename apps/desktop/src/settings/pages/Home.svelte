<script lang="ts">
  import { onMount, untrack } from "svelte";
  import {
    errorMessage, formatBytes, getStats, historyStorage, listMicrophones, modelsInstalled, openPrivacyPane, permissions,
    type HistoryStorage, type InstalledModel, type Stats,
  } from "../../lib/api";
  import { uiLocale } from "../../lib/format";
  import { guard } from "../../lib/guard.svelte";
  import { t } from "../../lib/i18n";
  import { defaultProfile } from "../../lib/labels";
  import { go } from "../../lib/route";
  import { engineLine, micLine, outputLine } from "../../lib/stages";
  import {
    dayDate, durationParts, fillDays, groupSums, heatLevel, localDay, percent, periodStart, speed, streaks, topWithOthers, type Period,
  } from "../../lib/statsView";
  import { client, config } from "../../lib/store.svelte";
  import BarList from "../../lib/ui/BarList.svelte";
  import Columns from "../../lib/ui/Columns.svelte";
  import Empty from "../../lib/ui/Empty.svelte";
  import Heatmap from "../../lib/ui/Heatmap.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Ring from "../../lib/ui/Ring.svelte";
  import Segmented from "../../lib/ui/Segmented.svelte";
  import StatTile from "../../lib/ui/StatTile.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  const PERIODS: Period[] = ["today", "week", "month", "all"];
  const PERIOD_KEY = "kongwe.stats.period";
  /** Days the heatmap covers. */
  const HEAT_DAYS = 140;
  /** Characters a minute the saved-time estimate assumes for typing. */
  const TYPING_SPEED = 40;

  function storedPeriod(): Period {
    try {
      const v = localStorage.getItem(PERIOD_KEY);
      return PERIODS.includes(v as Period) ? (v as Period) : "month";
    } catch { return "month"; }
  }

  let period = $state<Period>(storedPeriod());
  let stats = $state<Stats | null>(null);
  let failed = $state(false);
  let storage = $state<HistoryStorage | null>(null);
  let models = $state<InstalledModel[] | null>(null);
  let mics = $state<string[]>([]);
  let access = $state<boolean | null>(null);
  let now = $state(new Date());

  async function load(of: Period = period) {
    now = new Date();
    try {
      stats = await getStats(periodStart(of, now), -now.getTimezoneOffset() * 60);
      failed = false;
    } catch (e) {
      if (stats === null) failed = true;
      toast.error(errorMessage(e));
    }
    historyStorage().then((s) => { storage = s; }).catch(() => { storage = null; });
    modelsInstalled().then((m) => { models = m; }).catch(() => { models = null; });
  }
  async function checkAccess() {
    try { access = (await permissions()).accessibility; } catch { access = null; }
  }

  onMount(() => {
    checkAccess();
    listMicrophones().then((m) => { mics = m; }).catch(() => { mics = []; });
    const onFocus = () => { checkAccess(); load(); };
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

  // Days used: always over all time.
  const heat = $derived.by(() => {
    const rows = stats?.active_days ?? [];
    const from = today - HEAT_DAYS + 1;
    const chars = fillDays(rows, from, today);
    const max = Math.max(0, ...chars);
    return {
      levels: chars.map((c) => heatLevel(c, max)),
      titles: chars.map((c, i) => t("home.days.cell", { date: dateOf(from + i), n: num(c) })),
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
    const first = period === "all" ? stats.days[0].day : localDay(new Date(periodStart(period, now) * 1000));
    const perDay = fillDays(stats.days, first, today);
    const weekly = perDay.length > 90;
    const values = weekly ? groupSums(perDay, 7) : perDay;
    const peak = stats.days.reduce((a, b) => (b.chars > a.chars ? b : a));
    const marks = [0, 0.25, 0.5, 0.75, 1].map((f) => dateOf(first + Math.round((perDay.length - 1) * f)));
    return {
      values, weekly, peak,
      labels: perDay.length > 4 ? marks : [dateOf(first), dateOf(today)],
      titles: values.map((v, i) => t("home.days.cell", { date: dateOf(first + i * (weekly ? 7 : 1)), n: num(v) })),
    };
  });

  const apps = $derived.by(() => {
    if (!stats || !totals) return [];
    const { top, others } = topWithOthers(stats.apps, 5);
    const rows = top.map((a) => ({ label: a.name || t("home.app_unknown"), value: a.count, note: share(a.count, totals.count), muted: !a.name }));
    if (others) rows.push({ label: t("home.apps.others", { n: String(others.names) }), value: others.count, note: share(others.count, totals.count), muted: true });
    return rows;
  });
  const profiles = $derived.by(() => {
    if (!stats || !totals) return [];
    return stats.profiles.map((p) => {
      const known = config.value?.profiles.find((x) => x.id === p.name);
      return { label: known ? known.name : `${p.name}${t("history.deleted_profile")}`, value: p.count, note: share(p.count, totals.count), muted: !known };
    });
  });
  const scripts = $derived.by(() => {
    if (!totals) return [];
    const whole = totals.han + totals.latin + totals.kana;
    return ([["han", totals.han], ["latin", totals.latin], ["kana", totals.kana]] as const)
      .filter(([, n]) => n > 0)
      .map(([k, n]) => ({ label: t(`home.lang.${k}`), value: n, note: `${percent(n, whole)}%` }));
  });

  const cpm = $derived(totals ? speed(totals.speed_chars, totals.speed_audio_ms) : null);
  const modelBytes = $derived((models ?? []).reduce((n, m) => n + m.size_bytes, 0));

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
      outputLine(p).name,
    ].join(t("home.flow_sep"));
  });
</script>

<PageHead title={t("nav.home")} description={t("home.description")}>
  {#snippet actions()}
    <Segmented options={PERIODS.map((p) => ({ value: p, label: t(`home.period.${p}`) }))} bind:value={period} label={t("home.period.label")} />
  {/snippet}
</PageHead>

{#if access === false}
  <div class="attention" role="alert">
    <Ring tone="live" />
    <span class="grow">{t("home.access_needed")}</span>
    <button type="button" class="btn" onclick={() => openPrivacyPane("accessibility").catch((e) => toast.error(errorMessage(e)))}>{t("general.open_system_settings")}</button>
  </div>
{/if}

{#if failed}
  <div class="card">
    <Empty text={t("home.load_failed")}>
      {#snippet action()}<button type="button" class="btn" onclick={() => load()}>{t("history.retry")}</button>{/snippet}
    </Empty>
  </div>
{:else if totals && stats}
  {#if empty}<p class="hint">{t("home.empty.intro")}</p>{/if}

  <div class="tiles">
    <StatTile parts={[{ value: num(totals.chars) }]} label={t("home.tile.chars")} detail={totals.count ? t("home.tile.chars_detail", { n: num(Math.round(totals.chars / totals.count)) }) : undefined} />
    <StatTile parts={[{ value: num(totals.count) }]} label={t("home.tile.count")} detail={periodDays.span > 1 && totals.count ? t("home.tile.count_detail", { n: num(Math.round(totals.count / periodDays.span)) }) : undefined} />
    <StatTile parts={span(totals.audio_ms)} label={t("home.tile.audio")} detail={t("home.tile.audio_detail")} />
    <StatTile parts={stats.saved_ms === null ? [{ value: t("home.none") }] : span(stats.saved_ms)} label={t("home.tile.saved")} detail={t("home.tile.saved_detail", { n: String(TYPING_SPEED) })} />
  </div>

  <div class="cards">
    <section class="card wide">
      <h2>{t("home.days.title")}<em>{t("home.days.range")}</em></h2>
      <p class="sub">{t("home.days.sub")}</p>
      <div class="heatwrap">
        <Heatmap levels={heat.levels} titles={heat.titles} />
        <div class="facts">
          <div><b class="num">{t("home.days.n", { n: String(runs.current) })}</b><span>{t("home.days.current")}</span></div>
          <div><b class="num">{t("home.days.n", { n: String(runs.longest) })}</b><span>{t("home.days.longest")}</span></div>
          <div><b class="num">{periodDays.used} / {periodDays.span}</b><span>{t("home.days.active")}</span></div>
        </div>
      </div>
    </section>

    <section class="card wide">
      <h2>{t(daily?.weekly ? "home.daily.title_weekly" : "home.daily.title")}{#if daily}<em>{t("home.daily.peak", { n: num(daily.peak.chars), date: dateOf(daily.peak.day) })}</em>{/if}</h2>
      <p class="sub">{t(daily?.weekly ? "home.daily.sub_weekly" : "home.daily.sub")}</p>
      {#if daily}<Columns values={daily.values} labels={daily.labels} titles={daily.titles} />{:else}<p class="none">{t("home.empty.block")}</p>{/if}
    </section>

    <section class="card">
      <h2>{t("home.apps.title")}{#if stats.apps.length}<em>{t("home.apps.total", { n: String(stats.apps.length) })}</em>{/if}</h2>
      <p class="sub">{t("home.apps.sub")}</p>
      {#if apps.length}<BarList rows={apps} />{:else}<p class="none">{t("home.empty.block")}</p>{/if}
    </section>

    <section class="card">
      <h2>{t("home.profiles.title")}</h2>
      <p class="sub">{t("home.profiles.sub")}</p>
      {#if profiles.length}<BarList rows={profiles} />{:else}<p class="none">{t("home.empty.block")}</p>{/if}
    </section>

    <section class="card">
      <h2>{t("home.hours.title")}</h2>
      <p class="sub">{t("home.hours.sub")}</p>
      {#if empty}<p class="none">{t("home.empty.block")}</p>{:else}
        <Columns values={stats.hours} height={72} labels={["0", "6", "12", "18", "23"]} titles={stats.hours.map((n, h) => t("home.hours.cell", { h: String(h), n: num(n) }))} />
      {/if}
    </section>

    <section class="card">
      <h2>{t("home.speed.title")}</h2>
      <p class="sub">{t("home.speed.sub")}</p>
      {#if cpm === null}<p class="none">{t("home.empty.speed")}</p>{:else}
        <dl class="kv">
          <dt>{t("home.speed.average")}</dt><dd>{t("home.speed.cpm", { n: num(cpm) })}</dd>
          <dt>{t("home.speed.fastest")}</dt><dd>{t("home.speed.cpm", { n: num(totals.max_cpm) })}</dd>
          <dt>{t("home.speed.clip")}</dt><dd>{secs(totals.timed_count ? totals.audio_ms / totals.timed_count : 0)}</dd>
          <dt>{t("home.speed.longest_audio")}</dt><dd>{spanText(totals.max_audio_ms)}</dd>
          <dt>{t("home.speed.longest_chars")}</dt><dd>{t("home.chars", { n: num(totals.max_chars) })}</dd>
        </dl>
      {/if}
    </section>

    <section class="card">
      <h2>{t("home.rewrite.title")}</h2>
      <p class="sub">{t("home.rewrite.sub")}</p>
      {#if empty}<p class="none">{t("home.empty.block")}</p>{:else}
        <dl class="kv">
          <dt>{t("home.rewrite.changed")}</dt><dd>{share(totals.changed, totals.count)}</dd>
          <dt>{t("home.rewrite.fixes")}</dt><dd>{t("home.times", { n: num(totals.dict_fixes) })}</dd>
          <dt>{t("home.rewrite.snippets")}</dt><dd>{t("home.times", { n: num(totals.snippets) })}</dd>
          <dt>{t("home.rewrite.none")}</dt><dd>{t("home.items", { n: num(totals.unrewritten) })}</dd>
          <dt>{t("home.rewrite.failed")}</dt><dd>{t("home.items", { n: num(totals.degraded) })}</dd>
        </dl>
      {/if}
    </section>

    <section class="card">
      <h2>{t("home.time.title")}</h2>
      <p class="sub">{t("home.time.sub")}</p>
      {#if empty}<p class="none">{t("home.empty.block")}</p>{:else}
        <dl class="kv">
          <dt>{t("home.time.average")}</dt><dd>{secs((totals.asr_ms + totals.llm_ms) / totals.count)}</dd>
          <dt>{t("home.time.asr")}</dt><dd>{secs(totals.asr_ms / totals.count)}</dd>
          <dt>{t("home.time.llm")}</dt><dd>{totals.llm_count ? secs(totals.llm_ms / totals.llm_count) : t("home.none")}</dd>
          <dt>{t("home.time.slowest")}</dt><dd>{secs(totals.max_total_ms)}</dd>
        </dl>
      {/if}
    </section>

    <section class="card">
      <h2>{t("home.lang.title")}</h2>
      <p class="sub">{t("home.lang.sub")}</p>
      {#if scripts.length}<BarList rows={scripts} />{:else}<p class="none">{t("home.empty.block")}</p>{/if}
    </section>

    <section class="card">
      <h2>{t("home.storage.title")}</h2>
      <p class="sub">{t("home.storage.sub")}</p>
      <dl class="kv">
        <dt>{t("home.storage.rows")}</dt><dd>{storage ? t("home.items", { n: num(storage.rows) }) : t("home.none")}</dd>
        <dt>{t("home.storage.recordings")}</dt><dd>{storage ? t("home.storage.count_size", { n: num(storage.recordings), size: formatBytes(storage.recording_bytes) }) : t("home.none")}</dd>
        <dt>{t("home.storage.models")}</dt><dd>{models ? t("home.storage.count_size", { n: num(models.length), size: formatBytes(modelBytes) }) : t("home.none")}</dd>
      </dl>
    </section>
  </div>

  {#if flowLine}
    <div class="status">
      <Ring tone="accent" />
      <span class="grow">{t("home.flow_line", { flow: flowLine })}</span>
      <button type="button" class="btn quiet" onclick={() => guard.request(() => go("flow"))}>{t("home.flow_button")}</button>
    </div>
  {/if}
{/if}

<style>
  .attention { display: flex; align-items: center; gap: 12px; margin-bottom: 22px; padding: 14px var(--row-x); border-radius: var(--radius-group); background: color-mix(in srgb, var(--live) 10%, var(--surface)); border: 1px solid color-mix(in srgb, var(--live) 35%, transparent); }
  .grow { flex: 1; min-width: 0; }
  .hint { margin: 0 4px 18px; color: var(--ink-2); }
  .tiles { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 14px; }
  .cards { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; margin-top: 14px; }
  .card { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); padding: var(--card-pad); min-width: 0; }
  .wide { grid-column: 1 / -1; }
  /* A wide window fits three cards in a row. */
  @container (min-width: 1040px) {
    .cards { grid-template-columns: repeat(3, minmax(0, 1fr)); }
  }
  h2 { margin: 0 0 4px; font-size: var(--text-lg); font-weight: 600; display: flex; align-items: baseline; gap: 12px; }
  h2 em { font-style: normal; font-weight: 400; font-size: var(--text-sm); color: var(--ink-2); margin-left: auto; text-align: right; }
  .sub { margin: 0 0 18px; color: var(--ink-2); font-size: var(--text-sm); }
  .none { margin: 0; color: var(--ink-3); font-size: var(--text-control); }
  .heatwrap { display: flex; gap: 36px; align-items: flex-start; }
  .facts { display: flex; flex-direction: column; gap: 12px; min-width: 0; }
  .facts b { display: block; font: 700 23px/1.15 var(--font-round); }
  .facts span { color: var(--ink-2); font-size: var(--text-sm); }
  .kv { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 11px 16px; margin: 0; font-size: var(--text-control); }
  .kv dt { color: var(--ink-2); }
  .kv dd { margin: 0; font-weight: 600; text-align: right; font-variant-numeric: tabular-nums; }
  .status { display: flex; align-items: center; gap: 12px; margin-top: 14px; padding: 14px var(--row-x); border-radius: var(--radius-group); background: var(--surface); border: 1px solid var(--line); font-size: var(--text-control); color: var(--ink-2); }
</style>
