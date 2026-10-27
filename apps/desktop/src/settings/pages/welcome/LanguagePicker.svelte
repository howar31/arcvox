<script lang="ts">
  import { onMount } from "svelte";
  import type { ClientSettings } from "../../../lib/api";
  import { uiLanguageSystem } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import Icon from "../../../lib/ui/Icon.svelte";

  type Choice = ClientSettings["ui_language"];

  /** A button that names the interface language and opens a list upward to pick another; a pick closes it. */
  let { value, onchange }: { value: Choice; onchange: (v: Choice) => void } = $props();

  let open = $state(false);
  let root = $state<HTMLElement>();
  let system = $state<string | null>(null);

  // Each language is named in itself; only "follow system" is in the interface language.
  const NAMES: Record<string, string> = { "zh-TW": "正體中文", "zh-tw": "正體中文", en: "English" };
  const labelOf = (v: Choice) => (v === "system" ? t("general.lang_system") : NAMES[v]);
  const groups: Choice[][] = [["system"], ["zh-tw", "en"]];

  function pick(v: Choice) {
    open = false;
    if (v !== value) onchange(v);
  }
  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && open) { e.preventDefault(); open = false; }
  }
  function onpointerdown(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }

  onMount(() => {
    uiLanguageSystem().then((tag) => { system = NAMES[tag] ?? null; }).catch(() => {});
  });
</script>

<svelte:window {onkeydown} {onpointerdown} />

<div class="picker" bind:this={root}>
  {#if open}
    <div class="pop" role="menu" aria-label={t("general.language")}>
      {#each groups as group, i (i)}
        {#if i > 0}<hr />{/if}
        {#each group as v (v)}
          <button type="button" role="menuitemradio" aria-checked={v === value} class:on={v === value} onclick={() => pick(v)}>
            <span class="mark">{#if v === value}<Icon name="check" size={13} />{/if}</span>{labelOf(v)}
            {#if v === "system" && system}<small>{system}</small>{/if}
          </button>
        {/each}
      {/each}
    </div>
  {/if}
  <button type="button" class="pbtn" class:open aria-expanded={open} aria-haspopup="menu" aria-label={t("general.language")} onclick={() => (open = !open)}>
    <Icon name="globe" />{labelOf(value)}
  </button>
</div>

<style>
  .picker { position: relative; }
  .pbtn { display: inline-flex; align-items: center; gap: 7px; height: var(--control); padding: 0 10px; border: 0; border-radius: var(--radius); background: none; color: var(--ink-2); font-size: var(--text-control); white-space: nowrap; cursor: default; }
  .pbtn:hover, .pbtn.open { background: var(--sunken); }
  .pop { position: absolute; bottom: calc(100% + 8px); left: 0; z-index: 20; display: flex; flex-direction: column; min-width: 220px; padding: 6px; border-radius: 14px; border: 1px solid var(--line); background: var(--surface); box-shadow: 0 10px 30px rgba(0,0,0,.14), var(--lift); color: var(--ink); }
  .pop button { display: flex; align-items: center; gap: 8px; height: 32px; padding: 0 10px 0 6px; border: 0; border-radius: 8px; background: none; color: var(--ink); font-size: var(--text-control); text-align: left; white-space: nowrap; cursor: default; }
  .pop button:hover { background: var(--sunken); }
  .pop button.on { font-weight: 600; }
  .pop small { margin-left: auto; padding-left: 16px; color: var(--ink-3); font-size: var(--text-sm); font-weight: 400; }
  .mark { width: 18px; flex: none; display: inline-grid; place-items: center; color: var(--accent); }
  hr { border: 0; border-top: 1px solid var(--line); margin: 5px 4px; }
</style>
