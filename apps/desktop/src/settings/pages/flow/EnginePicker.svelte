<script lang="ts">
  import type { ManifestView, ProviderConfig } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import { engineDescription, engineKind, type Installed } from "../../../lib/stages";
  import Tag from "../../../lib/ui/Tag.svelte";

  /**
   * A list of engines to choose one from. `follow`, when given, adds a first choice that follows the
   * default engine (selected as ""); `none` adds a last choice that selects no engine (selected as null).
   */
  let { engines, selected, onpick, label, manifest, installed, follow, none, stale, suggested }: {
    engines: ProviderConfig[];
    selected: string | null;
    onpick: (id: string | null) => void;
    /** Accessible name of the group. */
    label: string;
    manifest: ManifestView | null;
    installed: Installed;
    follow?: { name: string; help: string };
    none?: { name: string; help: string };
    /** Shown as the selected row when the stored engine is not in `engines`. */
    stale?: { name: string; help: string };
    /** The engine the profile's template was tuned with; it gets the suggested tag. */
    suggested?: string | null;
  } = $props();
</script>

<div class="picker" role="radiogroup" aria-label={label}>
  {#if follow}
    <button type="button" class="pick" role="radio" aria-checked={selected === "" && !stale} onclick={() => onpick("")}>
      <span class="dot" class:on={selected === "" && !stale}></span>
      <span class="text"><span class="name">{follow.name}</span><small>{follow.help}</small></span>
    </button>
  {/if}
  {#if stale}
    <div class="pick stale" role="radio" aria-checked="true" aria-disabled="true" tabindex="-1">
      <span class="dot on"></span>
      <span class="text"><span class="name">{stale.name}<Tag tone="live">{t("home.not_found")}</Tag></span><small>{stale.help}</small></span>
    </div>
  {/if}
  {#each engines as p (p.id)}
    {@const kind = engineKind(p, installed)}
    <button type="button" class="pick" role="radio" aria-checked={selected === p.id} onclick={() => onpick(p.id)}>
      <span class="dot" class:on={selected === p.id}></span>
      <span class="text">
        <span class="name">{p.label}{#if p.id === suggested}<Tag tone="accent" title={t("flow.suggested_tip")}>{t("flow.suggested")}</Tag>{/if}</span>
        <small class:warn={kind.missing}>{#if p.kind !== "http"}{kind.meta}<span class="sep" aria-hidden="true">·</span>{/if}{engineDescription(p, manifest)}</small>
      </span>
    </button>
  {/each}
  {#if none}
    <button type="button" class="pick" role="radio" aria-checked={selected === null && !stale} onclick={() => onpick(null)}>
      <span class="dot" class:on={selected === null && !stale}></span>
      <span class="text"><span class="name">{none.name}</span><small>{none.help}</small></span>
    </button>
  {/if}
</div>

<style>
  .picker { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); overflow: hidden; }
  .pick { display: flex; align-items: center; gap: 16px; width: 100%; min-height: var(--row-min); padding: var(--row-y) var(--row-x); border: 0; background: none; text-align: left; cursor: default; }
  .pick + .pick { border-top: 1px solid var(--line); }
  button.pick:hover { background: var(--sunken); }
  .pick:focus-visible { outline-offset: -2px; }
  .dot { width: 20px; height: 20px; border-radius: 50%; border: 2px solid var(--line-strong); flex: none; display: grid; place-items: center; }
  .dot.on { border-color: var(--accent); }
  .dot.on::after { content: ""; width: 10px; height: 10px; border-radius: 50%; background: var(--accent); }
  .stale .dot.on { border-color: var(--live); }
  .stale .dot.on::after { background: var(--live); }
  .text { flex: 1; min-width: 0; }
  .name { display: flex; align-items: center; gap: 8px; font-weight: 600; overflow-wrap: anywhere; }
  small { display: block; margin-top: 2px; color: var(--ink-2); font-size: var(--text-sm); overflow-wrap: anywhere; }
  small.warn { color: var(--warn); }
  .sep { margin: 0 6px; }
</style>
