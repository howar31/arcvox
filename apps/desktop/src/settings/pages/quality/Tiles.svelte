<script lang="ts">
  /** The four figures of one engine's run: word and punctuation error rates, seconds per row, failed rows. */
  import type { EvalSummary } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import { perRow, rate, scored } from "../../../lib/qualityView";
  let { summary }: { summary: EvalSummary } = $props();
</script>

<div class="tiles">
  <div class="tile"><small>{t("quality.tile.words")}</small><b class="num">{#if scored(summary.total.text)}{rate(summary.total.text)}<em>%</em>{:else}<span class="none">{t("quality.no_reference")}</span>{/if}</b></div>
  <div class="tile"><small>{t("quality.tile.punct")}</small><b class="num">{#if scored(summary.total.punctuation)}{rate(summary.total.punctuation)}<em>%</em>{:else}<span class="none">{t("quality.no_reference")}</span>{/if}</b></div>
  <div class="tile"><small>{t("quality.tile.per_row")}</small><b class="num">{perRow(summary)}<em>{t("common.seconds")}</em></b></div>
  <div class="tile"><small>{t("quality.tile.failed")}</small><b class="num" class:bad={summary.failed > 0}>{summary.failed}</b></div>
</div>

<style>
  .tiles { display: grid; grid-template-columns: repeat(4, 1fr); }
  .tile { padding: 14px var(--row-x); border-left: 1px solid var(--line); min-width: 0; }
  .tile:first-child { border-left: 0; }
  small { display: block; color: var(--ink-2); font-size: var(--text-sm); }
  b { display: block; font: 600 var(--text-stat)/1.2 var(--font-round); margin-top: 2px; white-space: nowrap; }
  b em { font-style: normal; font: 500 var(--text-sm)/1 var(--font); color: var(--ink-2); margin-left: 4px; }
  b.bad { color: var(--live); }
  .none { font: 500 var(--text)/1.2 var(--font); color: var(--ink-3); }
</style>
