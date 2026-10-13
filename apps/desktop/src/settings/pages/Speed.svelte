<script lang="ts">
  import { onMount } from "svelte";
  import { benchClipFromHistory, benchClipStatus, benchHistoryCandidates, benchRecordStart, benchRecordStop, errorMessage, onBenchLevel, type ClipCandidate, type ClipStatus, type ProviderConfig } from "../../lib/api";
  import { bench } from "../../lib/bench.svelte";
  import { formatDateTime } from "../../lib/format";
  import { t } from "../../lib/i18n";
  import { providerLabel } from "../../lib/labels";
  import { config } from "../../lib/store.svelte";
  import Group from "../../lib/ui/Group.svelte";
  import Icon from "../../lib/ui/Icon.svelte";
  import Meter from "../../lib/ui/Meter.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import Ring from "../../lib/ui/Ring.svelte";
  import Row from "../../lib/ui/Row.svelte";
  import Tag from "../../lib/ui/Tag.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  // The engine caps a clip at this length but keeps the microphone open until stopped.
  const MAX_RECORD_SECS = 30;

  let clip = $state<ClipStatus | null>(null);
  let recording = $state(false);
  let elapsed = $state(0);
  let levelBars = $state<number[]>(Array(12).fill(0.1));
  let candidates = $state<ClipCandidate[]>([]);
  let pickedCandidate = $state<number | null>(null);
  // The run itself is kept in the store: it goes on while another page is shown.
  const running = $derived(bench.running);
  // An engine deleted since it was ticked no longer counts.
  const selectedAsr = $derived((bench.asr ?? []).filter((id) => asrProviders.some((p) => p.id === id)));
  const selectedLlm = $derived((bench.llm ?? []).filter((id) => llmProviders.some((p) => p.id === id)));

  const asrProviders = $derived((config.value?.providers ?? []).filter((p) => p.capabilities.transcribe));
  const llmProviders = $derived((config.value?.providers ?? []).filter((p) => p.capabilities.chat));
  const canStart = $derived(!!clip?.present && selectedAsr.length + selectedLlm.length > 0 && !running && !recording);

  const KNOWN_ERRORS: Record<string, string> = {
    "no speech in the recording": "speed.err.no_speech",
    "recording not kept": "speed.err.recording_not_kept",
    "no test clip": "speed.err.no_clip",
    "not recording": "speed.err.not_recording",
  };
  function speedError(e: unknown): string {
    const message = errorMessage(e);
    const key = KNOWN_ERRORS[message];
    return key ? t(key) : message;
  }
  const fail = (e: unknown) => { toast.error(speedError(e)); };

  const when = formatDateTime;
  function secs(ms: number) { return (ms / 1000).toFixed(2); }
  function truncate(text: string, max = 40) { return text.length > max ? `${text.slice(0, max)}…` : text; }

  // Level events arrive once per audio block; render at most one bar per animation frame.
  let pendingRms: number | null = null;
  let frame = 0;
  function onLevel(rms: number) {
    pendingRms = Math.max(pendingRms ?? 0, rms);
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      if (pendingRms === null) return;
      levelBars = [...levelBars.slice(1), Math.min(1, 0.1 + pendingRms * 4)];
      pendingRms = null;
    });
  }

  let timer: ReturnType<typeof setInterval> | undefined;
  function stopTimer() {
    if (timer) clearInterval(timer);
    timer = undefined;
  }

  onMount(() => {
    bench.watch();
    // The first visit ticks the default profile's engines; later visits keep the choice.
    if (bench.asr === null || bench.llm === null) {
      const pair = config.value?.default_engines;
      const ids = new Set((config.value?.providers ?? []).map((p) => p.id));
      bench.asr = pair && ids.has(pair.asr) ? [pair.asr] : [];
      bench.llm = pair && ids.has(pair.llm) ? [pair.llm] : [];
    }
    benchClipStatus().then((c) => { clip = c; }).catch(fail);
    benchHistoryCandidates().then((c) => {
      candidates = c;
      pickedCandidate = c[0]?.id ?? null;
    }).catch(fail);

    let disposed = false;
    const unlisteners: (() => void)[] = [];
    const keep = (p: Promise<() => void>) => p.then((u) => { if (disposed) u(); else unlisteners.push(u); });
    keep(onBenchLevel(onLevel));
    return () => {
      disposed = true;
      unlisteners.forEach((u) => u());
      if (frame) cancelAnimationFrame(frame);
      stopTimer();
      // Leaving the page must not leave the microphone open.
      if (recording) benchRecordStop().catch(() => {});
    };
  });

  async function record() {
    try {
      await benchRecordStart();
      recording = true;
      elapsed = 0;
      levelBars = Array(12).fill(0.1);
      const started = Date.now();
      stopTimer();
      timer = setInterval(() => {
        elapsed = Math.floor((Date.now() - started) / 1000);
        if (elapsed >= MAX_RECORD_SECS) stopRecording();
      }, 250);
    } catch (e) { fail(e); }
  }

  async function stopRecording() {
    if (!recording) return;
    stopTimer();
    recording = false;
    try { clip = await benchRecordStop(); } catch (e) { fail(e); }
  }

  async function useCandidate() {
    if (pickedCandidate === null) return;
    try { clip = await benchClipFromHistory(pickedCandidate); } catch (e) { fail(e); }
  }

  function toggle(list: string[], id: string, on: boolean): string[] {
    return on ? (list.includes(id) ? list : [...list, id]) : list.filter((x) => x !== id);
  }

  async function start() {
    const defaultAsr = config.value?.default_engines.asr;
    const asr = selectedAsr.includes(defaultAsr ?? "")
      ? [defaultAsr as string, ...selectedAsr.filter((id) => id !== defaultAsr)]
      : [...selectedAsr];
    const llm = [...selectedLlm];
    try { await bench.start(asr, llm, t("speed.sentence")); } catch (e) { fail(e); }
  }

  async function cancel() {
    try { await bench.cancel(); } catch (e) { fail(e); }
  }

  const clipTone = $derived(recording ? "live" : clip?.present ? "ok" : "off");
  const clipText = $derived(
    recording ? t("speed.recording")
    : clip?.present ? t("speed.clip_info", { secs: clip.secs.toFixed(1), time: clip.recorded_at === null ? "" : when(clip.recorded_at) })
    : t("speed.no_clip"),
  );
</script>

<PageHead title={t("speed.title")} description={t("speed.description")} crumb={{ label: t("nav.engines"), page: "engines" }} />

<Group title={t("speed.clip")}>
  <div class="read">
    <span class="sub">{t("speed.read_aloud")}</span>
    <blockquote>{t("speed.sentence")}</blockquote>
  </div>
  <Row label={clipText}>
    {#snippet leading()}<Ring tone={clipTone} />{/snippet}
    {#if recording}
      <Meter levels={levelBars} />
      <span class="sub num elapsed">{t("speed.elapsed", { secs: String(elapsed), max: String(MAX_RECORD_SECS) })}</span>
      <button type="button" class="btn" onclick={stopRecording}>{t("speed.stop_recording")}</button>
    {:else}
      <button type="button" class="btn" onclick={record} disabled={running}><Icon name="mic" size={14} />{t("speed.record")}</button>
    {/if}
  </Row>
  {#if candidates.length > 0}
    <Row label={t("speed.from_history")}>
      <select class="pick" bind:value={pickedCandidate} disabled={recording || running} aria-label={t("speed.from_history")}>
        {#each candidates as c (c.id)}
          <option value={c.id}>{truncate(c.raw_text)} ({when(c.ts)})</option>
        {/each}
      </select>
      <button type="button" class="btn" onclick={useCandidate} disabled={pickedCandidate === null || recording || running}>{t("speed.use_recording")}</button>
    </Row>
  {/if}
</Group>

{#snippet engineList(id: string, title: string, list: ProviderConfig[], selected: string[], set: (next: string[]) => void)}
  {#if list.length > 0}
    <div class="sublist" role="group" aria-labelledby={id}>
      <div class="subhead" id={id}>{title}</div>
      {#each list as p (p.id)}
        <Row>
          <label class="check">
            <input type="checkbox" checked={selected.includes(p.id)} disabled={running}
              onchange={(e) => set(toggle(selected, p.id, e.currentTarget.checked))} />
            <span>{providerLabel(config.value, p.id)}</span>
          </label>
        </Row>
      {/each}
    </div>
  {/if}
{/snippet}

<Group title={t("speed.engines")}>
  {@render engineList("speed-asr", t("speed.role.asr"), asrProviders, selectedAsr, (next) => { bench.asr = next; })}
  {@render engineList("speed-llm", t("speed.role.llm"), llmProviders, selectedLlm, (next) => { bench.llm = next; })}
</Group>

<div class="run">
  <span class="sub caution">{t("speed.warning")}</span>
  {#if running}
    <span class="sub num" role="status">{t("speed.remaining", { n: String(bench.remaining) })}</span>
    <button type="button" class="btn" onclick={cancel}>{t("speed.stop")}</button>
  {/if}
  <button type="button" class="btn primary" onclick={start} disabled={!canStart}>{t("speed.start")}</button>
</div>

{#if bench.runs.length > 0}
  <Group title={t("speed.results")}>
    <div class="cols sub" aria-hidden="true">
      <span class="grow">{t("speed.col.engine")}</span>
      <span class="val">{t("speed.col.load")}</span>
      <span class="val">{t("speed.col.infer")}</span>
    </div>
    {#each bench.runs as run, i (i)}
      <Row>
        <div class="result">
          <div class="title">
            <span class="name">{providerLabel(config.value, run.provider_id)}</span>
            <Tag>{t(`speed.role.${run.role}`)}</Tag>
            <Tag>{run.cold ? t("speed.cold") : t("speed.warm")}</Tag>
          </div>
          {#if run.error}<small class="out error" title={run.error}>{run.error}</small>
          {:else}<small class="out" title={run.output}>{run.output}</small>{/if}
        </div>
        <span class="val num"><span class="sr-only">{t("speed.col.load")} </span>{secs(run.load_ms)}</span>
        <span class="val num"><span class="sr-only">{t("speed.col.infer")} </span>{secs(run.infer_ms)}</span>
      </Row>
    {/each}
  </Group>
{/if}

<style>
  .read { display: flex; flex-direction: column; gap: 8px; padding: 14px var(--row-x); border-bottom: 1px solid var(--line); }
  blockquote { margin: 0; padding: 12px 16px; border-left: 3px solid var(--accent); border-radius: var(--radius); background: var(--accent-soft); font-size: var(--text-lg); line-height: 1.6; }
  .elapsed { min-width: 72px; text-align: right; }
  .pick { width: 200px; flex: none; }
  .sublist + .sublist { border-top: 1px solid var(--line); }
  .subhead { padding: 12px var(--row-x) 0; font-size: var(--text-sm); font-weight: 600; color: var(--ink-2); }
  .check { flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; }
  .check span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .run { display: flex; align-items: center; gap: 10px; margin-top: 18px; }
  .caution { flex: 1; min-width: 0; }
  .cols { display: flex; gap: 14px; padding: 12px var(--row-x) 2px; font-weight: 600; }
  .grow { flex: 1; }
  .val { width: 92px; flex: none; text-align: right; }
  .result { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .title { display: flex; flex-wrap: wrap; align-items: center; gap: 2px 6px; min-width: 0; }
  .title .name { min-width: 0; overflow-wrap: anywhere; }
  .out { color: var(--ink-2); font-size: var(--text-sm); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .out.error { color: var(--live); }
</style>
