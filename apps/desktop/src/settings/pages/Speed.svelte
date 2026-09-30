<script lang="ts">
  import { onMount } from "svelte";
  import { benchCancel, benchClipFromHistory, benchClipStatus, benchHistoryCandidates, benchRecordStart, benchRecordStop, benchStart, errorMessage, onBenchDone, onBenchLevel, onBenchProgress, type BenchRun, type ClipCandidate, type ClipStatus } from "../../lib/api";
  import { t } from "../../lib/i18n";
  import { defaultProfile, providerLabel } from "../../lib/labels";
  import { config } from "../../lib/store.svelte";

  // The engine caps a clip at this length but keeps the microphone open until stopped.
  const MAX_RECORD_SECS = 30;

  let clip = $state<ClipStatus | null>(null);
  let recording = $state(false);
  let elapsed = $state(0);
  let levelBars = $state<number[]>(Array(12).fill(0.1));
  let candidates = $state<ClipCandidate[]>([]);
  let pickedCandidate = $state<number | null>(null);
  let selectedAsr = $state<string[]>([]);
  let selectedLlm = $state<string[]>([]);
  let running = $state(false);
  let remaining = $state(0);
  let runs = $state<BenchRun[]>([]);
  let status = $state("");

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

  function when(ts: number) { return new Date(ts * 1000).toLocaleString(); }
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
    const profile = defaultProfile(config.value);
    const ids = new Set((config.value?.providers ?? []).map((p) => p.id));
    if (profile && ids.has(profile.asr_provider)) selectedAsr = [profile.asr_provider];
    if (profile?.llm_provider && ids.has(profile.llm_provider)) selectedLlm = [profile.llm_provider];
    benchClipStatus().then((c) => { clip = c; }).catch((e) => { status = speedError(e); });
    benchHistoryCandidates().then((c) => {
      candidates = c;
      pickedCandidate = c[0]?.id ?? null;
    }).catch((e) => { status = speedError(e); });

    let disposed = false;
    const unlisteners: (() => void)[] = [];
    const keep = (p: Promise<() => void>) => p.then((u) => { if (disposed) u(); else unlisteners.push(u); });
    keep(onBenchLevel(onLevel));
    keep(onBenchProgress(({ run, remaining: n }) => {
      runs = [...runs, run];
      remaining = n;
    }));
    keep(onBenchDone(() => { running = false; remaining = 0; }));
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
    status = "";
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
    } catch (e) { status = speedError(e); }
  }

  async function stopRecording() {
    if (!recording) return;
    stopTimer();
    recording = false;
    try { clip = await benchRecordStop(); } catch (e) { status = speedError(e); }
  }

  async function useCandidate() {
    if (pickedCandidate === null) return;
    status = "";
    try { clip = await benchClipFromHistory(pickedCandidate); } catch (e) { status = speedError(e); }
  }

  function toggle(list: string[], id: string, on: boolean): string[] {
    return on ? (list.includes(id) ? list : [...list, id]) : list.filter((x) => x !== id);
  }

  async function start() {
    const defaultAsr = defaultProfile(config.value)?.asr_provider;
    const asr = selectedAsr.includes(defaultAsr ?? "")
      ? [defaultAsr as string, ...selectedAsr.filter((id) => id !== defaultAsr)]
      : [...selectedAsr];
    const llm = [...selectedLlm];
    status = "";
    runs = [];
    running = true;
    remaining = 2 * (asr.length + llm.length);
    try {
      await benchStart(asr, llm, t("speed.sentence"));
    } catch (e) {
      running = false;
      remaining = 0;
      status = speedError(e);
    }
  }

  async function cancel() {
    try { await benchCancel(); } catch (e) { status = speedError(e); }
  }
</script>

<h2>{t("speed.title")}</h2>

<section>
  <h3>{t("speed.clip")}</h3>
  <p class="muted">
    {#if clip?.present}
      {t("speed.clip_info", { secs: clip.secs.toFixed(1), time: clip.recorded_at === null ? "" : when(clip.recorded_at) })}
    {:else}
      {t("speed.no_clip")}
    {/if}
  </p>
  <p>{t("speed.read_aloud")}</p>
  <blockquote class="sentence">{t("speed.sentence")}</blockquote>
  <div class="row">
    {#if recording}
      <button class="primary" onclick={stopRecording}>{t("speed.stop_recording")}</button>
      <div class="meter live">
        {#each levelBars as b}<span style="height: {Math.round(b * 100)}%"></span>{/each}
      </div>
      <span class="muted">{elapsed} / {MAX_RECORD_SECS}</span>
    {:else}
      <button onclick={record} disabled={running}>{t("speed.record")}</button>
    {/if}
  </div>
  {#if candidates.length > 0}
    <div class="row">
      <span>{t("speed.from_history")}</span>
      <select bind:value={pickedCandidate} disabled={recording || running}>
        {#each candidates as c (c.id)}
          <option value={c.id}>{truncate(c.raw_text)} ({when(c.ts)})</option>
        {/each}
      </select>
      <button onclick={useCandidate} disabled={pickedCandidate === null || recording || running}>{t("speed.use_recording")}</button>
    </div>
  {/if}
</section>

<section>
  <h3>{t("speed.providers")}</h3>
  <div class="groups">
    <fieldset>
      <legend>{t("speed.role.asr")}</legend>
      {#each asrProviders as p (p.id)}
        <label><input type="checkbox" checked={selectedAsr.includes(p.id)} disabled={running}
          onchange={(e) => { selectedAsr = toggle(selectedAsr, p.id, e.currentTarget.checked); }} /> {providerLabel(config.value, p.id)}</label>
      {/each}
    </fieldset>
    <fieldset>
      <legend>{t("speed.role.llm")}</legend>
      {#each llmProviders as p (p.id)}
        <label><input type="checkbox" checked={selectedLlm.includes(p.id)} disabled={running}
          onchange={(e) => { selectedLlm = toggle(selectedLlm, p.id, e.currentTarget.checked); }} /> {providerLabel(config.value, p.id)}</label>
      {/each}
    </fieldset>
  </div>
</section>

<section>
  <p class="muted">{t("speed.warning")}</p>
  <div class="row">
    <button class="primary" onclick={start} disabled={!canStart}>{t("speed.start")}</button>
    {#if running}
      <button onclick={cancel}>{t("speed.stop")}</button>
      <span class="muted">{t("speed.remaining", { n: String(remaining) })}</span>
    {/if}
  </div>
  <p class="status">{status}</p>
</section>

{#if runs.length > 0}
  <table>
    <thead><tr><th>{t("speed.col.provider")}</th><th>{t("speed.col.role")}</th><th>{t("speed.col.run")}</th><th>{t("speed.col.load")}</th><th>{t("speed.col.infer")}</th><th>{t("speed.col.output")}</th></tr></thead>
    <tbody>
      {#each runs as run, i (i)}
        <tr>
          <td>{providerLabel(config.value, run.provider_id)}</td>
          <td>{t(`speed.role.${run.role}`)}</td>
          <td>{run.cold ? t("speed.cold") : t("speed.warm")}</td>
          <td>{secs(run.load_ms)}</td>
          <td>{secs(run.infer_ms)}</td>
          <td class="output">
            {#if run.error}<span class="muted error" title={run.error}>{run.error}</span>
            {:else}<span title={run.output}>{run.output}</span>{/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}

<style>
  .sentence { margin: 8px 0; padding: 10px 14px; border-left: 3px solid var(--accent); border-radius: 6px; background: rgba(10, 132, 255, 0.08); font-size: 15px; }
  .groups { display: flex; gap: 16px; flex-wrap: wrap; }
  fieldset { flex: 1; min-width: 180px; border: 1px solid var(--border); border-radius: 8px; padding: 6px 12px; }
  legend { color: var(--muted); padding: 0 4px; }
  td.output { max-width: 280px; }
  td.output span { display: block; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .error { color: #d9534f; }
  .meter { display: flex; align-items: center; gap: 3px; height: 28px; width: 72px; }
  .meter span { flex: 1; background: var(--muted); border-radius: 2px; transition: height 60ms linear; }
  .meter.live span { background: #34c759; }
</style>
