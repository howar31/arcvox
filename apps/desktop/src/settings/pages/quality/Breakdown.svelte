<script lang="ts">
  /** One engine's results in detail: punctuation by kind, totals per category, and every row worst first. */
  import { MARK_KINDS, type EvalItem, type EvalSummary } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import { rate, worstFirst } from "../../../lib/qualityView";
  import Group from "../../../lib/ui/Group.svelte";
  import Tag from "../../../lib/ui/Tag.svelte";
  import TextDiff from "../../../lib/ui/TextDiff.svelte";

  let { summary, items, suffix }: { summary: EvalSummary; items: EvalItem[]; /** The engine's name, after each group title when there are several engines. */ suffix: string } = $props();

  const FIRST = 3;
  let shown = $state(FIRST);
  const rows = $derived(worstFirst(items, summary.provider));
  const head = (key: string) => suffix ? `${t(key)}（${suffix}）` : t(key);
  function categoryName(c: string): string {
    const key = `quality.category.${c}`;
    return t(key) === key ? c : t(key);
  }
  function pct(ms: number): string { return (ms / 1000).toFixed(1); }
</script>

<Group title={head("quality.marks")}>
  <table>
    <thead><tr><th></th>{#each MARK_KINDS as k (k)}<th class="n">{t(`quality.mark.${k}`)}</th>{/each}</tr></thead>
    <tbody>
      <tr><td class="k">{t("quality.marks.extra")}</td>{#each MARK_KINDS as k (k)}<td class="n num">{summary.total.marks[k].extra}</td>{/each}</tr>
      <tr><td class="k">{t("quality.marks.missing")}</td>{#each MARK_KINDS as k (k)}<td class="n num">{summary.total.marks[k].missing}</td>{/each}</tr>
    </tbody>
  </table>
</Group>

<Group title={head("quality.by_category")}>
  <table>
    <thead><tr><th>{t("quality.col.category")}</th><th class="n">{t("quality.col.rows")}</th><th class="n">{t("quality.col.words")}</th><th class="n">{t("quality.col.punct")}</th></tr></thead>
    <tbody>
      {#each summary.by_category as [name, totals] (name)}
        <tr><td class="k">{categoryName(name)}</td><td class="n num">{totals.items}</td><td class="n num">{rate(totals.text)}%</td><td class="n num">{rate(totals.punctuation)}%</td></tr>
      {/each}
    </tbody>
  </table>
</Group>

<Group title={head("quality.items")}>
  {#each rows.slice(0, shown) as it (it.id)}
    <div class="item">
      <div class="h">
        <span class="num">#{it.id}</span>
        {#each it.tags as tag (tag)}<Tag>{categoryName(tag)}</Tag>{/each}
        <span class="sp"></span>
        {#if it.error}
          <span class="bad">{t("quality.row_failed", { error: it.error })}</span>
        {:else}
          <span class="num">{t("quality.row_words", { p: rate(it.text) })}</span>
          <span class="num">{t("quality.row_punct", { p: rate(it.punctuation) })}</span>
          <span class="num">{t("quality.row_secs", { s: pct(it.ms) })}</span>
        {/if}
      </div>
      <div class="line"><span class="lab">{t("quality.expected")}</span><TextDiff from={null} to={it.expected} /></div>
      {#if !it.error}<div class="line"><span class="lab">{t("quality.output")}</span><TextDiff from={it.expected} to={it.output} /></div>{/if}
    </div>
  {/each}
  {#if rows.length > shown}
    <div class="foot"><button type="button" class="btn quiet" onclick={() => (shown = rows.length)}>{t("quality.show_rest", { n: String(rows.length - shown) })}</button></div>
  {:else if rows.length > FIRST}
    <div class="foot"><button type="button" class="btn quiet" onclick={() => (shown = FIRST)}>{t("quality.show_fewer")}</button></div>
  {/if}
</Group>

<style>
  table { width: 100%; border-collapse: collapse; font-size: var(--text-sm); }
  th { text-align: left; font-weight: 600; color: var(--ink-2); font-size: var(--text-xs); padding: 10px var(--row-x) 6px; }
  td { padding: 8px var(--row-x); border-top: 1px solid var(--line); }
  th.n, td.n { text-align: right; }
  td.k { font-weight: 500; }
  .item { padding: 12px var(--row-x); border-top: 1px solid var(--line); }
  .item:first-child { border-top: 0; }
  .h { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; font-size: var(--text-xs); color: var(--ink-2); margin-bottom: 6px; }
  .h .sp { flex: 1; }
  .bad { color: var(--live); }
  .line { display: flex; gap: 10px; align-items: baseline; }
  .line .lab { flex: none; width: 58px; color: var(--ink-3); font-size: var(--text-xs); }
  .line :global(.diff) { flex: 1; min-width: 0; }
  .foot { display: flex; justify-content: center; padding: 8px; border-top: 1px solid var(--line); }
</style>
