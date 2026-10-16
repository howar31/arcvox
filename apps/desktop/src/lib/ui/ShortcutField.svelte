<script lang="ts">
  import { tip } from "../tip";
  import { onMount } from "svelte";
  import { errorMessage, onShortcutCapture, shortcutCapture, shortcutCheck } from "../api";
  import { t } from "../i18n";
  import { keyLabel, shortcutLabel } from "../shortcut";
  import { pause } from "../pause.svelte";
  import Icon from "./Icon.svelte";

  /** The recording shortcuts: a list that is edited by pressing the keys to add. */
  let { value = $bindable() }: { value: string[][] } = $props();

  let capturing = $state(false);
  // The keys held while recording, and why the last combination was refused.
  let held = $state<string[]>([]);
  let refused = $state("");

  async function start() {
    refused = "";
    held = [];
    try {
      if (await shortcutCapture(true)) capturing = true;
      else refused = t("general.shortcut_unavailable");
    } catch (e) { refused = errorMessage(e); }
  }
  function stop() {
    if (!capturing) return;
    capturing = false;
    held = [];
    shortcutCapture(false).catch(() => {});
  }
  async function finish(keys: string[]) {
    held = [];
    let why: string | null;
    try { why = await shortcutCheck(keys, $state.snapshot(value)); } catch (e) { refused = errorMessage(e); return; }
    if (why) {
      // Recording goes on, so another combination can be tried at once.
      refused = t("shortcut.refuse." + why, { keys: shortcutLabel(keys) });
      return;
    }
    value.push(keys);
    stop();
  }

  onMount(() => {
    pause.watch();
    const unlisten = onShortcutCapture((p) => {
      if (!capturing) return;
      if (p.cancelled) { capturing = false; held = []; return; }
      if (p.done) { finish(p.keys); return; }
      refused = "";
      held = p.keys;
    });
    // Leaving the window ends the recording, so keys are not held back for nothing.
    const onBlur = () => stop();
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("blur", onBlur);
      stop();
      unlisten.then((f) => f());
    };
  });
</script>

<div class="keys">
  {#each value as keys, i (keys.join("+"))}
    <span class="combo">
      {#each keys as k, j (k)}{#if j > 0}<span class="plus" aria-hidden="true">+</span>{/if}<kbd>{keyLabel(k)}</kbd>{/each}
      <button type="button" class="x" disabled={value.length === 1} aria-label={t("common.remove") + " " + shortcutLabel(keys)} use:tip={t("common.remove")} onclick={() => { value.splice(i, 1); refused = ""; }}><Icon name="x" size={12} /></button>
    </span>
  {/each}
  {#if capturing}
    <span class="capture" role="status">
      {#if held.length}
        <span class="live">{#each held as k, j (k)}{#if j > 0}<span class="plus" aria-hidden="true">+</span>{/if}<kbd>{keyLabel(k)}</kbd>{/each}</span>
      {:else}
        <span>{t("general.shortcut_press")}</span>
      {/if}
      <button type="button" class="btn" onclick={stop}>{t("common.cancel")}</button>
    </span>
  {:else}
    <button type="button" class="btn" disabled={pause.value} onclick={start}><Icon name="plus" size={14} />{t("general.shortcut_add")}</button>
  {/if}
</div>
{#if pause.value}
  <p class="refuse note">{t("general.shortcut_paused")}</p>
{:else}
  <p class="refuse" role="alert">{refused}</p>
{/if}

<style>
  .keys { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; margin-top: 14px; }
  .combo { display: inline-flex; align-items: center; gap: 6px; height: 40px; padding: 0 6px 0 10px; border-radius: 12px; border: 1px solid var(--line-strong); background: var(--surface); }
  kbd { display: inline-flex; align-items: center; height: 26px; padding: 0 9px; border-radius: 7px; background: var(--sunken); border: 1px solid var(--line); font: 600 var(--text-sm)/1 var(--font); white-space: nowrap; }
  .plus { color: var(--ink-3); font-size: var(--text-sm); }
  .x { display: grid; place-items: center; width: 26px; height: 26px; padding: 0; border: 0; border-radius: 13px; background: none; color: var(--ink-3); cursor: default; }
  .x:hover { background: var(--sunken); color: var(--ink); }
  .x:disabled { opacity: .35; pointer-events: none; }
  .capture { display: inline-flex; align-items: center; gap: 10px; height: 40px; padding: 0 6px 0 14px; border-radius: 12px; border: 2px dashed var(--accent); background: var(--accent-soft); color: var(--accent); font-size: var(--text-control); }
  .live { display: inline-flex; align-items: center; gap: 6px; }
  .capture .btn { height: 28px; }
  .refuse { margin: 10px 0 0; min-height: 20px; color: var(--live); font-size: var(--text-sm); }
  .refuse.note { color: var(--ink-3); }
</style>
