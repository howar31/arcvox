<script lang="ts">
  import { onMount } from "svelte";
  import { onWidgetLevel, onWidgetMessage, onWidgetState, uiLanguage, widgetCopy, widgetDismiss, type MessagePayload, type WidgetState } from "../lib/api";
  import { setLanguage, t, type Lang } from "../lib/i18n";

  // A local named "state" collides with the $state rune during type checking.
  let widgetState = $state<WidgetState>("idle");
  let level = $state(0);
  let bars = $state<number[]>(Array(12).fill(0.1));
  let message = $state<MessagePayload | null>(null);

  onMount(() => {
    uiLanguage().then((l) => setLanguage(l as Lang));
    const unsubs = [
      onWidgetState((p) => { setLanguage(p.language); widgetState = p.state; if (p.state !== "idle") message = null; }),
      onWidgetLevel((p) => { level = p.rms; bars = [...bars.slice(1), Math.min(1, 0.1 + p.rms * 4)]; }),
      onWidgetMessage((p) => { setLanguage(p.language); message = p; }),
    ];
    return () => { unsubs.forEach((u) => u.then((f) => f())); };
  });

  const label = $derived.by(() => {
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

<div class="panel" class:error={message?.kind === "error"}>
  {#if message}
    <div class="message">
      <div class="text">{t(message.key)}</div>
      {#if message.detail}<div class="detail">{message.detail}</div>{/if}
      {#if message.copyable}
        <div class="copy-row">
          <div class="copyable">{message.copyable}</div>
          <button onclick={() => widgetCopy(message!.copyable!)}>{t("widget.copy")}</button>
          <button class="dismiss" onclick={dismiss} aria-label={t("widget.dismiss")}>✕</button>
        </div>
      {/if}
    </div>
  {:else}
    <div class="meter" class:live={widgetState === "recording" || widgetState === "locked"}>
      {#each bars as b}<span style="height: {Math.round(b * 100)}%"></span>{/each}
    </div>
    <div class="label" class:busy={widgetState === "processing" || widgetState === "inserting"}>{label}</div>
  {/if}
</div>

<style>
  .panel { box-sizing: border-box; width: 100vw; height: 100vh; padding: 10px 14px; border-radius: 14px; background: var(--panel); color: var(--panel-fg); display: flex; align-items: center; gap: 12px; font-size: 13px; overflow: hidden; }
  .panel.error { background: rgba(120, 30, 30, 0.94); }
  .meter { display: flex; align-items: center; gap: 3px; height: 28px; width: 72px; }
  .meter span { flex: 1; background: rgba(255, 255, 255, 0.35); border-radius: 2px; transition: height 60ms linear; }
  .meter.live span { background: #34c759; }
  .label { flex: 1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .label.busy { opacity: 0.8; animation: pulse 1.2s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: 0.4; } }
  .message { flex: 1; display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .detail { font-size: 11px; opacity: 0.75; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .copy-row { display: flex; align-items: center; gap: 8px; }
  .copyable { flex: 1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-family: ui-monospace, monospace; }
  button { font: inherit; padding: 2px 10px; border-radius: 8px; border: 1px solid rgba(255,255,255,0.3); background: rgba(255,255,255,0.12); color: inherit; }
  button.dismiss { padding: 2px 8px; }
</style>
