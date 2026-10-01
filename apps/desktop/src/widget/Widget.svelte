<script lang="ts">
  import { onMount } from "svelte";
  import { onWidgetLevel, onWidgetMessage, onWidgetState, onWidgetTranscript, uiLanguage, widgetCopy, widgetDismiss, widgetDragStart, type MessagePayload, type WidgetState } from "../lib/api";
  import { setLanguage, t, type Lang } from "../lib/i18n";
  import Icon from "../lib/ui/Icon.svelte";

  // A local named "state" collides with the $state rune during type checking.
  let widgetState = $state<WidgetState>("idle");
  let level = $state(0);
  let message = $state<MessagePayload | null>(null);
  // Raw transcript shown while the rewrite runs.
  let transcript = $state("");
  // At a left or right edge of the display the window is tall and the pill is vertical.
  const portrait = window.matchMedia("(orientation: portrait)");
  let vertical = $state(portrait.matches);

  onMount(() => {
    // Until the language is known the widget stays in English; each event carries it too.
    uiLanguage().then((l) => setLanguage(l as Lang)).catch(() => {});
    const onShape = () => { vertical = portrait.matches; };
    portrait.addEventListener("change", onShape);
    const unsubs = [
      onWidgetState((p) => {
        setLanguage(p.language);
        widgetState = p.state;
        if (p.state !== "idle" && p.state !== "placing") message = null;
        if (p.state !== "processing" && p.state !== "inserting") transcript = "";
        if (p.state !== "recording" && p.state !== "locked") level = 0;
      }),
      onWidgetLevel((p) => { level = p.rms; }),
      onWidgetMessage((p) => { setLanguage(p.language); message = p; transcript = ""; }),
      onWidgetTranscript((p) => { transcript = p.text; }),
    ];
    return () => {
      portrait.removeEventListener("change", onShape);
      unsubs.forEach((u) => u.then((f) => f()));
    };
  });

  const listening = $derived(widgetState === "recording" || widgetState === "locked");
  // Halo around the live ring, in px: grows with the input level, clamped.
  const halo = $derived(listening ? 2 + 7 * Math.min(1, Math.max(0, level * 4)) : 0);
  const tone = $derived.by(() => {
    if (message) return message.kind === "error" ? "live" : "accent";
    if (listening) return "live";
    if (widgetState === "processing") return "think";
    if (widgetState === "inserting" || widgetState === "placing") return "accent";
    return "off";
  });
  const label = $derived.by(() => {
    // The vertical pill has room for a short label only.
    if (widgetState === "processing" && transcript && !vertical) return transcript;
    switch (widgetState) {
      case "recording": return t("widget.listening");
      case "locked": return t(vertical ? "widget.locked_short" : "widget.locked");
      case "processing": return t("widget.processing");
      case "inserting": return t("widget.inserting");
      case "placing": return t("widget.placing");
      default: return "";
    }
  });

  // The pill can be dragged anywhere that is not a button; the app moves the window.
  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0 || (e.target as Element).closest("button")) return;
    widgetDragStart().catch(() => {});
  }

  function dismiss() {
    widgetDismiss();
    message = null;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="pill" {onpointerdown}>
  <span class="ring {tone}" style:--halo="{halo}px" aria-hidden="true"></span>
  {#if message}
    {@const second = message.copyable ?? message.detail}
    <div class="text">
      <span class="line wrap" title={t(message.key)}>{t(message.key)}</span>
      {#if second}<small class:mono={!!message.copyable && message.kind !== "error"} title={second}>{second}</small>{/if}
    </div>
    {#if message.copyable}
      <!-- On a failure the copy is of the detail, a secondary action: an icon leaves the message its room. -->
      {#if message.kind === "error"}
        <button type="button" class="close" onclick={() => widgetCopy(message!.copyable!).catch(() => {})} aria-label={t("widget.copy")} title={t("widget.copy")}><Icon name="copy" size={14} /></button>
      {:else}
        <button type="button" class="copy" onclick={() => widgetCopy(message!.copyable!).catch(() => {})}>{t("widget.copy")}</button>
      {/if}
      <button type="button" class="close" onclick={dismiss} aria-label={t("widget.dismiss")} title={t("widget.dismiss")}><Icon name="x" size={14} /></button>
    {/if}
  {:else}
    <div class="text">
      <span class="line" title={widgetState === "processing" && transcript ? transcript : undefined}>{label}</span>
    </div>
  {/if}
</div>

<style>
  .pill { width: 100vw; height: 100vh; border-radius: 32px; padding: 0 16px 0 22px; display: flex; align-items: center; gap: 12px; overflow: hidden; background: var(--pill-bg); color: var(--pill-ink); box-shadow: inset 0 0 0 1px var(--pill-line); font-size: 13px; line-height: 1.35; user-select: none; -webkit-user-select: none; cursor: default; }
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
  /* Vertical: the ring on top, a short label written downwards. */
  @media (orientation: portrait) {
    .pill { flex-direction: column; padding: 14px 0; gap: 10px; }
    .text { min-height: 0; align-items: center; }
    .line { writing-mode: vertical-rl; max-height: 100%; letter-spacing: .06em; }
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) {
    /* Without the spin, a full ring: a still gap would read as a fault. */
    .ring.think { animation: none; border-right-color: var(--pill-accent); }
    .ring.live { box-shadow: 0 0 0 5px var(--pill-live-halo); transition: none; }
  }
</style>
