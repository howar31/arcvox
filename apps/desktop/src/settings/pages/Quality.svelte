<script lang="ts">
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { errorMessage, qualityClear, qualityDelete, qualityList, qualitySourceInfo, type ProviderConfig, type QualitySource, type ReportHead, type SourceInfo } from "../../lib/api";
  import { formatDateTime } from "../../lib/format";
  import { periodRange } from "../../lib/historyView";
  import { t } from "../../lib/i18n";
  import { profileName, providerLabel } from "../../lib/labels";
  import { quality } from "../../lib/quality.svelte";
  import { fraction, providersOf } from "../../lib/qualityView";
  import { config } from "../../lib/store.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import NumberField from "../../lib/ui/NumberField.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Segmented from "../../lib/ui/Segmented.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import { toast } from "../../lib/ui/toast.svelte";
  import PeriodPicker from "./history/PeriodPicker.svelte";
  import Compare from "./quality/Compare.svelte";
  import Result from "./quality/Result.svelte";
  import Tiles from "./quality/Tiles.svelte";

  const fail = (e: unknown) => { toast.error(errorMessage(e)); };

  let heads = $state<ReportHead[]>([]);
  let info = $state<SourceInfo | null>(null);
  let stopping = $state(false);
  // A deletion asks once more: the button becomes the question for a few seconds.
  let sureDelete = $state<string | null>(null);
  let sureTimer: ReturnType<typeof setTimeout> | undefined;

  const asrProviders = $derived((config.value?.providers ?? []).filter((p) => p.capabilities.transcribe));
  const llmProviders = $derived((config.value?.providers ?? []).filter((p) => p.capabilities.chat));
  const profiles = $derived(config.value?.profiles ?? []);
  const profile = $derived(profiles.find((p) => p.id === quality.profile) ?? profiles.find((p) => p.is_default) ?? null);
  const running = $derived(quality.running);
  const run = $derived(quality.run);
  // Engines deleted since they were ticked no longer count.
  const selectedAsr = $derived((quality.asr ?? []).filter((id) => asrProviders.some((p) => p.id === id)));
  const selectedLlm = $derived((quality.llm ?? []).filter((id) => llmProviders.some((p) => p.id === id)));
  const needsAsr = $derived(quality.stage !== "llm");
  const needsLlm = $derived(quality.stage !== "asr");
  const source = $derived.by((): QualitySource | null => {
    if (quality.source === "file") return quality.file ? { kind: "file", path: quality.file } : null;
    const { since, until } = periodRange(quality.period, new Date());
    return { kind: "history", since, until, limit: Math.max(0, Math.floor(quality.limit)) };
  });
  const canStart = $derived(!!source && (info?.rows ?? 0) > 0 && !!profile && !running
    && (quality.stage === "full" ? selectedAsr.length <= 1 && selectedLlm.length <= 1 : quality.stage === "asr" ? selectedAsr.length > 0 : selectedLlm.length > 0));

  // The default profile's engines are ticked on the first visit; later visits keep the choice.
  onMount(() => {
    quality.watch();
    if (quality.profile === null) quality.profile = profiles.find((p) => p.is_default)?.id ?? null;
    if (quality.asr === null || quality.llm === null) {
      const pair = config.value?.default_engines;
      const ids = new Set((config.value?.providers ?? []).map((p) => p.id));
      quality.asr = pair && ids.has(pair.asr) ? [pair.asr] : [];
      quality.llm = pair && ids.has(pair.llm) ? [pair.llm] : [];
    }
    loadHeads();
    return () => clearTimeout(sureTimer);
  });
  $effect(() => {
    const s = source;
    info = null;
    if (!s) return;
    qualitySourceInfo(s).then((i) => { info = i; }).catch(fail);
  });

  async function loadHeads() {
    try { heads = await qualityList(); } catch (e) { fail(e); }
  }
  async function chooseFile() {
    try {
      const picked = await openDialog({ multiple: false, directory: false, filters: [{ name: "TSV", extensions: ["tsv", "txt"] }] });
      if (typeof picked === "string") quality.file = picked;
    } catch (e) { fail(e); }
  }
  function toggle(list: string[], id: string, on: boolean): string[] {
    return on ? (list.includes(id) ? list : [...list, id]) : list.filter((x) => x !== id);
  }
  async function start() {
    if (!source || !profile) return;
    stopping = false;
    try {
      await quality.start({ source, stage: quality.stage, profile: profile.id, asr: needsAsr ? selectedAsr : [], llm: needsLlm ? selectedLlm : [] });
    } catch (e) { fail(e); }
  }
  async function stop() {
    stopping = true;
    try { await quality.cancel(); } catch (e) { fail(e); }
  }
  async function discard() {
    try { await qualityClear(); await quality.refresh(); } catch (e) { fail(e); }
  }
  function openReport(file: string) {
    quality.opened = file;
    quality.show("result");
  }
  function askDelete(file: string) {
    clearTimeout(sureTimer);
    sureDelete = file;
    sureTimer = setTimeout(() => { sureDelete = null; }, 4000);
  }
  async function deleteReport(file: string) {
    clearTimeout(sureTimer);
    sureDelete = null;
    try { await qualityDelete(file); await loadHeads(); } catch (e) { fail(e); }
  }
  function startCompare() {
    quality.show("compare");
  }
  const engineNames = (h: ReportHead) => h.providers.flatMap((p) => providersOf(p, h.stage)).map((id) => providerLabel(config.value, id)).join("、");
  const rowsOf = (h: ReportHead) => Math.round(h.items / Math.max(1, h.providers.length));
  const stageOf = (s: string) => t(`quality.stage.${s}`);
  const runEngine = $derived(run?.provider ? providersOf(run.provider, run.stage).map((id) => providerLabel(config.value, id)).join(" + ") : "");
</script>

{#snippet engineList(id: string, title: string, list: ProviderConfig[], selected: string[], set: (next: string[]) => void, one: boolean)}
  {#if list.length > 0}
    <div class="sublist" role="group" aria-labelledby={id}>
      <div class="subhead" id={id}>{title}</div>
      {#each list as p (p.id)}
        <Row>
          <label class="check">
            <input type={one ? "radio" : "checkbox"} name={one ? id : undefined} checked={selected.includes(p.id)} disabled={running}
              onchange={(e) => set(one ? (e.currentTarget.checked ? [p.id] : []) : toggle(selected, p.id, e.currentTarget.checked))} />
            <span>{providerLabel(config.value, p.id)}</span>
            {#if profile && (id === "quality-asr" ? profile.asr_provider ?? config.value?.default_engines.asr : profile.llm_provider ?? config.value?.default_engines.llm) === p.id}<Tag tone="accent">{t("quality.profile_engine")}</Tag>{/if}
          </label>
        </Row>
      {/each}
    </div>
  {/if}
{/snippet}

{#if quality.view === "result"}
  <Result onsaved={loadHeads} />
{:else if quality.view === "compare"}
  <Compare {heads} />
{:else}
  <PageHead title={t("quality.title")} description={t("quality.description")} crumb={{ label: t("nav.engines"), page: "engines" }} />

  {#if run && (running || run.error)}
    <Group title={running ? t("quality.running") : t("quality.result")}>
      <Row label={t("quality.running_row", { stage: stageOf(run.stage), profile: profileName(config.value, run.profile), n: String(run.rows) })}
        help={run.error ? t("quality.run_failed", { error: run.error }) : runEngine ? t("quality.running_on", { engine: runEngine, done: String(run.done) }) : undefined}>
        {#if running}
          <span class="sub num" role="status">{run.done} / {run.total}</span>
          <button type="button" class="btn" onclick={stop} disabled={stopping}>{stopping ? t("quality.stopping") : t("quality.stop")}</button>
        {:else}
          <button type="button" class="btn quiet" onclick={discard}>{t("common.dismiss")}</button>
        {/if}
      </Row>
      {#if running}<div class="prog" aria-hidden="true"><i style="width:{fraction(run.done, run.total) * 100}%"></i></div>{/if}
    </Group>
    {#if run.summaries.length > 0}
      <Group title={t("quality.so_far")}>
        {#each run.summaries as s (s.provider)}
          {#if run.summaries.length > 1}<div class="subhead">{providersOf(s.provider, run.stage).map((id) => providerLabel(config.value, id)).join(" + ")}</div>{/if}
          <Tiles summary={s} />
        {/each}
      </Group>
    {/if}
  {:else}
    {#if run && !run.running}
      <Group>
        <Row label={t("quality.last_run")} help={t("quality.last_run_help", { stage: stageOf(run.stage), n: String(run.rows) })}>
          <button type="button" class="btn quiet" onclick={() => { quality.opened = null; quality.show("result"); }}>{t("quality.open")}</button>
        </Row>
      </Group>
    {/if}

    <Group title={t("quality.data")} info={t("quality.data_info")}>
      <Row label={t("quality.source")}>
        <Segmented options={[{ value: "history", label: t("quality.source.history") }, { value: "file", label: t("quality.source.file") }]} bind:value={quality.source} label={t("quality.source")} />
      </Row>
      {#if quality.source === "history"}
        <Row label={t("quality.period")}>
          <PeriodPicker value={quality.period} withAudio onchange={(v) => (quality.period = v)} />
        </Row>
        <Row label={t("quality.limit")} help={t("quality.limit_help")}>
          <NumberField bind:value={quality.limit} min={0} max={100000} unit={t("quality.rows_unit")} label={t("quality.limit")} />
        </Row>
        <Row label={info ? t("quality.available", { n: String(info.rows) }) : t("quality.counting")} help={info ? t("quality.available_help", { n: String(info.available), heard: String(info.heard) }) : undefined} />
      {:else}
        <Row label={t("quality.file")} help={quality.file ?? t("quality.no_file")}>
          <button type="button" class="btn" onclick={chooseFile} disabled={running}>{t("quality.choose_file")}</button>
        </Row>
        {#if quality.file}
          <Row label={info ? t("quality.file_rows", { n: String(info.rows) }) : t("quality.counting")} />
        {/if}
      {/if}
    </Group>

    <Group title={t("quality.part")}>
      <Row label={t("quality.stage")} help={t("quality.stage_help")}>
        <Segmented options={[{ value: "asr", label: t("quality.stage.asr") }, { value: "llm", label: t("quality.stage.llm") }, { value: "full", label: t("quality.stage.full") }]} bind:value={quality.stage} label={t("quality.stage")} />
      </Row>
      <Row label={t("quality.profile")} help={t("quality.profile_help")}>
        <select class="field" bind:value={() => profile?.id ?? "", (v) => (quality.profile = v)} aria-label={t("quality.profile")} disabled={running}>
          {#each profiles as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
        </select>
      </Row>
    </Group>

    <Group title={t("quality.engines")} help={quality.stage === "full" ? t("quality.engines_full_help") : undefined}>
      {#if needsAsr}{@render engineList("quality-asr", t("speed.role.asr"), asrProviders, selectedAsr, (next) => { quality.asr = next; }, quality.stage === "full")}{/if}
      {#if needsLlm}{@render engineList("quality-llm", t("speed.role.llm"), llmProviders, selectedLlm, (next) => { quality.llm = next; }, quality.stage === "full")}{/if}
    </Group>

    <div class="run">
      <span class="sub caution">{t("quality.warning")}</span>
      <button type="button" class="btn primary" onclick={start} disabled={!canStart}>{t("quality.start")}</button>
    </div>
  {/if}

  <Group title={t("quality.saved")}>
    {#if heads.length === 0}
      <p class="help">{t("quality.none_saved")}</p>
    {:else}
      {#each heads as h (h.file)}
        <Row label={h.name} help={t("quality.saved_meta", { time: formatDateTime(Number(h.created)), stage: stageOf(h.stage), engines: engineNames(h), n: String(rowsOf(h)) })}>
          {#snippet tag()}{#if h.origin === "command"}<Tag>{t("quality.origin.command")}</Tag>{/if}{#if h.stopped}<Tag tone="warn">{t("quality.stopped")}</Tag>{/if}{/snippet}
          <span class="stat num">{t("quality.saved_scores", { words: (h.text.length ? (h.text.errors / h.text.length) * 100 : 0).toFixed(1), punct: (h.punctuation.length ? (h.punctuation.errors / h.punctuation.length) * 100 : 0).toFixed(1) })}</span>
          <button type="button" class="btn quiet" onclick={() => openReport(h.file)}>{t("quality.open")}</button>
          {#if sureDelete === h.file}
            <button type="button" class="btn danger" onclick={() => deleteReport(h.file)}>{t("quality.delete_sure")}</button>
          {:else}
            <button type="button" class="btn quiet danger" onclick={() => askDelete(h.file)}>{t("quality.delete")}</button>
          {/if}
        </Row>
      {/each}
      {#if heads.length > 1}
        <div class="foot"><button type="button" class="btn quiet" onclick={startCompare}>{t("quality.compare")}</button></div>
      {/if}
    {/if}
  </Group>
{/if}

<style>
  .sublist + .sublist { border-top: 1px solid var(--line); }
  .subhead { padding: 12px var(--row-x) 0; font-size: var(--text-sm); font-weight: 600; color: var(--ink-2); }
  .check { flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; }
  .check span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .field { width: 240px; flex: none; }
  .run { display: flex; align-items: center; gap: 10px; margin-top: 18px; }
  .caution { flex: 1; min-width: 0; }
  .prog { height: 6px; border-radius: 3px; background: var(--sunken); overflow: hidden; margin: 0 var(--row-x) 14px; }
  .prog i { display: block; height: 100%; background: var(--accent); transition: width .3s; }
  .stat { color: var(--ink-2); font-size: var(--text-sm); white-space: nowrap; }
  .foot { display: flex; justify-content: center; padding: 8px; border-top: 1px solid var(--line); }
  .help { margin: 0; padding: 14px var(--row-x); }
</style>
