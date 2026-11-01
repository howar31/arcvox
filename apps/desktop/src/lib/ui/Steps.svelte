<script lang="ts">
  import { t } from "../i18n";

  /** The numbered steps of a form, each a button that goes to it. */
  let { steps, current, onpick }: {
    steps: { id: string; label: string; disabled?: boolean }[];
    current: string;
    onpick: (id: string) => void;
  } = $props();
</script>

<ol class="steps" aria-label={t("engines.steps")}>
  {#each steps as s, i (s.id)}
    <li><button type="button" class:on={s.id === current} disabled={s.disabled} onclick={() => onpick(s.id)}><i>{i + 1}</i>{s.label}</button></li>
  {/each}
</ol>

<style>
  .steps { display: flex; align-items: center; gap: 14px; margin: 0; padding: 0; list-style: none; font-size: var(--text-sm); color: var(--ink-3); }
  .steps li { display: inline-flex; align-items: center; }
  .steps button { display: inline-flex; align-items: center; gap: 7px; padding: 2px 6px 2px 2px; border: 0; border-radius: 13px; background: none; color: var(--ink-3); font: inherit; font-size: var(--text-sm); cursor: default; }
  .steps button:not([disabled]):hover { background: var(--sunken); color: var(--ink); }
  .steps button.on { color: var(--ink); font-weight: 600; }
  .steps li + li::before { content: "›"; margin: 0 8px; color: var(--ink-3); }
  .steps i { display: inline-grid; place-items: center; width: 22px; height: 22px; border-radius: 50%; background: var(--sunken); font-style: normal; font-size: var(--text-xs); font-weight: 600; color: var(--ink-2); }
  .steps button.on i { background: var(--accent); color: var(--accent-ink); }
</style>
