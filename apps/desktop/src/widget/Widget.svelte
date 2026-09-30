<script lang="ts">
  import { onMount } from "svelte";
  import { onWidgetLevel, onWidgetMessage, onWidgetState, onWidgetTranscript, uiLanguage, widgetCopy, widgetDismiss, type MessagePayload, type WidgetState } from "../lib/api";
  import { setLanguage, t, type Lang } from "../lib/i18n";
  import Icon from "../lib/ui/Icon.svelte";

  // A local named "state" collides with the $state rune during type checking.
  let widgetState = $state<WidgetState>("idle");
  let level = $state(0);
  let message = $state<MessagePayload | null>(null);
  // Raw transcript shown while the rewrite runs.
  let transcript = $state("");

  onMount(() => {
    // Until the language is known the widget stays in English; each event carries it too.
    uiLanguage().then((l) => setLanguage(l as Lang)).catch(() => {});
    const unsubs = [
      onWidgetState((p) => {
        setLanguage(p.language);
        widgetState = p.state;
        if (p.state !== "idle") message = null;
        if (p.state !== "processing" && p.state !== "inserting") transcript = "";
        if (p.state !== "recording" && p.state !== "locked") level = 0;
      }),
      onWidgetLevel((p) => { level = p.rms; }),
      onWidgetMessage((p) => { setLanguage(p.language); message = p; transcript = ""; }),
      onWidgetTranscript((p) => { transcript = p.text; }),
    ];
    return () => { unsubs.forEach((u) => u.then((f) => f())); };
  });

  const listening = $derived(widgetState === "recording" || widgetState === "locked");
  // Halo around the live ring, in px: grows with the input level, clamped.
  const halo = $derived(listening ? 2 + 7 * Math.min(1, Math.max(0, level * 4)) : 0);
  const tone = $derived.by(() => {
    if (message) return message.kind === "error" ? "live" : "accent";
    if (listening) return "live";
    if (widgetState === "processing") return "think";
    if (widgetState === "inserting") return "accent";
    return "off";
  });
  const label = $derived.by(() => {
    if (widgetState === "processing" && transcript) return transcript;
    switch (widgetState) {
      case "recording": return t("widget.listening");
      case "locked": return t("widget.locked");
      case "processing": return t("widget.processing");
      case "inserting": return t("widget.inserting");
      default: return "";
    }
  });

  function dismiss() {
    widgetDismiss();
    message = null;
  }
</script>

<div class="pill">
  <span class="ring {tone}" style:--halo="{halo}px" aria-hidden="true"></span>
  {#if message}
    {@const second = message.copyable ?? message.detail}
    <div class="text">
      <span class="line wrap" title={t(message.key)}>{t(message.key)}</span>
      {#if second}<small class:mono={!!message.copyable} title={second}>{second}</small>{/if}
    </div>
    {#if message.copyable}
      <button type="button" class="copy" onclick={() => widgetCopy(message!.copyable!).catch(() => {})}>{t("widget.copy")}</button>
      <button type="button" class="close" onclick={dismiss} aria-label={t("widget.dismiss")} title={t("widget.dismiss")}><Icon name="x" size={14} /></button>
    {/if}
  {:else}
    <div class="text">
      <span class="line" title={widgetState === "processing" && transcript ? transcript : undefined}>{label}</span>
    </div>
  {/if}
</div>

<style>
  .pill { width: 100vw; height: 100vh; border-radius: 32px; padding: 0 16px 0 22px; display: flex; align-items: center; gap: 12px; overflow: hidden; background: var(--pill-bg); color: var(--pill-ink); box-shadow: inset 0 0 0 1px var(--pill-line); font-size: 13px; line-height: 1.35; }
  .ring { flex: none; width: 20px; height: 20px; border-radius: 50%; border: 3px solid var(--pill-ink-2); }
  .ring.live { border-color: var(--pill-live); box-shadow: 0 0 0 var(--halo) var(--pill-live-halo); transition: box-shadow 80ms linear; }
  .ring.accent { border-color: var(--pill-accent); }
  .ring.think { border-color: var(--pill-accent); border-right-color: transparent; animation: spin .9s linear infinite; }
  .text { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .line, small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  /* A message may take two lines; the pill has room for them and the detail. */
  .line.wrap { white-space: normal; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; line-height: 1.3; }
  small { font-size: 11px; color: var(--pill-ink-2); }
  small.mono { font-family: var(--font-mono); }
  button { flex: none; display: inline-flex; align-items: center; justify-content: center; height: 26px; border: 0; border-radius: 13px; background: var(--pill-button); color: var(--pill-ink); cursor: default; }
  button:hover { background: var(--pill-button-hover); }
  button:focus-visible { outline-color: var(--pill-accent); }
  .copy { padding: 0 12px; }
  .close { width: 26px; margin-left: -4px; background: none; color: var(--pill-ink-2); }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) {
    /* Without the spin, a full ring: a still gap would read as a fault. */
    .ring.think { animation: none; border-right-color: var(--pill-accent); }
    .ring.live { box-shadow: 0 0 0 5px var(--pill-live-halo); transition: none; }
  }
</style>
