<script lang="ts" module>
  // Only one recording plays at a time.
  let playing: HTMLAudioElement | null = null;
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { errorMessage, historyAudio } from "../../../lib/api";
  import { clipLength } from "../../../lib/historyView";
  import { t } from "../../../lib/i18n";
  import Icon from "../../../lib/ui/Icon.svelte";
  import { toast } from "../../../lib/ui/toast.svelte";
  import { parseWav, peaks, type WavInfo } from "../../../lib/wav";

  /** Plays the recording of the history row `id`. */
  let { id, autoplay = false }: { id: number; autoplay?: boolean } = $props();

  /** Width of one bar and the space after it; a wider player shows more bars, not wider ones. */
  const BAR = 3;
  const GAP = 2;
  let audio: HTMLAudioElement | null = null;
  let wave: HTMLDivElement | undefined = $state();
  let width = $state(0);
  // The clip, kept to draw it again at another width.
  let clip = $state.raw<{ bytes: ArrayBuffer; info: WavInfo } | null>(null);
  let lengthMs = $state(0);
  let positionMs = $state(0);
  let active = $state(false);
  let failed = $state(false);
  let ready = $state(false);

  onMount(() => {
    let url: string | null = null;
    let disposed = false;
    historyAudio(id).then((bytes) => {
      if (disposed) return;
      const info = parseWav(bytes);
      if (!info) { failed = true; return; }
      clip = { bytes, info };
      lengthMs = info.durationMs;
      url = URL.createObjectURL(new Blob([bytes], { type: "audio/wav" }));
      const el = new Audio(url);
      el.addEventListener("timeupdate", () => { positionMs = el.currentTime * 1000; });
      el.addEventListener("play", () => { active = true; });
      el.addEventListener("pause", () => { active = false; });
      el.addEventListener("ended", () => { active = false; positionMs = 0; el.currentTime = 0; });
      audio = el;
      ready = true;
      if (autoplay) toggle();
    }).catch((e) => { failed = true; toast.error(errorMessage(e)); });
    const observer = new ResizeObserver((entries) => { width = entries[0].contentRect.width; });
    if (wave) observer.observe(wave);
    return () => {
      disposed = true;
      observer.disconnect();
      audio?.pause();
      if (playing === audio) playing = null;
      if (url) URL.revokeObjectURL(url);
    };
  });

  function toggle() {
    if (!audio) return;
    if (!audio.paused) { audio.pause(); return; }
    if (playing && playing !== audio) playing.pause();
    playing = audio;
    // A refused start leaves the control on "play".
    audio.play().catch(() => { active = false; });
  }

  function seek(e: MouseEvent) {
    if (!audio || lengthMs <= 0) return;
    const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const at = Math.min(1, Math.max(0, (e.clientX - box.left) / box.width));
    audio.currentTime = (at * lengthMs) / 1000;
    positionMs = at * lengthMs;
  }
  function onkeydown(e: KeyboardEvent) {
    if (!audio || lengthMs <= 0) return;
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    audio.currentTime = Math.min(lengthMs / 1000, Math.max(0, audio.currentTime + step));
    positionMs = audio.currentTime * 1000;
  }

  const count = $derived(Math.max(1, Math.floor((width + GAP) / (BAR + GAP))));
  // Drawn against the loudest part, so a quiet recording still shows its shape.
  const bars = $derived.by(() => {
    if (!clip) return new Array<number>(count).fill(0);
    const raw = peaks(clip.bytes, clip.info, count);
    const top = Math.max(...raw);
    return top > 0 ? raw.map((p) => p / top) : raw;
  });
  const played = $derived(lengthMs > 0 ? Math.round((positionMs / lengthMs) * count) : 0);
</script>

{#if failed}
  <p class="missing">{t("history.audio_missing")}</p>
{:else}
  <div class="player">
    <button type="button" class="toggle" disabled={!ready} aria-label={t(active ? "history.pause" : "history.play")} onclick={toggle}><Icon name={active ? "pause" : "play"} size={15} /></button>
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="wave" bind:this={wave} role="slider" tabindex="0" aria-label={t("history.recording")}
      aria-valuemin={0} aria-valuemax={Math.round(lengthMs / 1000)} aria-valuenow={Math.round(positionMs / 1000)}
      onclick={seek} {onkeydown}
    >
      {#each bars as b, i (i)}<i class:on={i < played} style:height="{Math.max(8, b * 100)}%" style:width="{BAR}px"></i>{/each}
    </div>
    <span class="time num">{clipLength(positionMs)} / {clipLength(lengthMs)}</span>
  </div>
{/if}

<style>
  .player { display: flex; align-items: center; gap: 14px; padding: 10px 16px 10px 10px; border-radius: 14px; background: var(--surface); border: 1px solid var(--line); }
  .toggle { width: 36px; height: 36px; padding: 0; border: 0; border-radius: 50%; background: var(--accent); color: var(--accent-ink); display: grid; place-items: center; flex: none; cursor: default; }
  .toggle:disabled { opacity: .5; }
  .toggle :global(.icon) { fill: currentColor; }
  .wave { flex: 1; min-width: 0; display: flex; align-items: center; justify-content: space-between; height: 32px; border-radius: 4px; overflow: hidden; }
  .wave i { flex: none; border-radius: 1.5px; background: var(--line-strong); }
  .wave i.on { background: var(--accent); }
  .time { color: var(--ink-2); font-size: var(--text-sm); white-space: nowrap; }
  .missing { margin: 0; color: var(--ink-2); font-size: var(--text-sm); }
</style>
