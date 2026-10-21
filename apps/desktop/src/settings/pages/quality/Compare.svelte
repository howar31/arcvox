<script lang="ts">
  /** Two saved reports side by side: the rates before and after, the tallies, and the rows that changed. */
  import { errorMessage, qualityCompare, type EvalChange, type EvalComparison, type ReportHead, type Tally } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import { quality } from "../../../lib/quality.svelte";
  import { rate } from "../../../lib/qualityView";
  import Group from "../../../lib/ui/Group.svelte";
  import PageHead from "../../../lib/ui/PageHead.svelte";
  import Tag from "../../../lib/ui/Tag.svelte";
  import TextDiff from "../../../lib/ui/TextDiff.svelte";
  import { toast } from "../../../lib/ui/toast.svelte";

  let { heads }: { heads: ReportHead[] } = $props();

  const FIRST = 3;
  let result = $state<EvalComparison | null>(null);
  let shown = $state(FIRST);
  const before = $derived(heads.find((h) => h.file === quality.compare.before) ?? null);
  // The second list offers the reports of the first one's stage.
  const afterChoices = $derived(before ? heads.filter((h) => h.stage === before.stage && h.file !== before.file) : []);
  const after = $derived(afterChoices.find((h) => h.file === quality.compare.after) ?? null);
  $effect(() => {
    const a = before?.file, b = after?.file;
    result = null;
    shown = FIRST;
    if (!a || !b) return;
    qualityCompare(a, b).then((c) => { result = c; }).catch((e) => toast.error(errorMessage(e)));
  });
  const matched = $derived(result ? result.words.improved + result.words.worse + result.words.same : 0);
  const rateOf = (s: EvalComparison["before"], which: "text" | "punctuation") => {
    const sum = s.reduce((acc, x) => ({ errors: acc.errors + x.total[which].errors, length: acc.length + x.total[which].length }), { errors: 0, length: 0 });
    return rate(sum);
  };
  function categoryName(c: string): string {
    const key = `quality.category.${c}`;
    return t(key) === key ? c : t(key);
  }
  const delta = (c: EvalChange, which: "words" | "punct") => which === "words" ? c.after.errors - c.before.errors : c.after_punctuation.errors - c.before_punctuation.errors;
  const signed = (n: number) => (n > 0 ? `+${n}` : String(n));
  const tone = (n: number) => (n > 0 ? "neg" : n < 0 ? "pos" : "zero");
</script>

{#snippet tally(tl: Tally)}<span class="pos num">{tl.improved}</span> / <span class="neg num">{tl.worse}</span> / <span class="num">{tl.same}</span>{/snippet}

<PageHead title={t("quality.compare.title")} description={t("quality.compare.description")} crumb={[{ label: t("nav.engines"), page: "engines" }, { label: t("quality.title"), page: "quality" }]} />

<Group>
  <div class="head">
    <select class="field" bind:value={() => quality.compare.before ?? "", (v) => (quality.compare = { before: v || null, after: null })} aria-label={t("quality.compare.before")}>
      <option value="">{t("quality.compare.pick")}</option>
      {#each heads as h (h.file)}<option value={h.file}>{h.name}（{t(`quality.stage.${h.stage}`)}）</option>{/each}
    </select>
    <span class="vs">{t("quality.compare.vs")}</span>
    <select class="field" bind:value={() => quality.compare.after ?? "", (v) => (quality.compare = { ...quality.compare, after: v || null })} aria-label={t("quality.compare.after")} disabled={!before}>
      <option value="">{t("quality.compare.pick")}</option>
      {#each afterChoices as h (h.file)}<option value={h.file}>{h.name}</option>{/each}
    </select>
    <span class="sp"></span>
    {#if result}<Tag>{t("quality.compare.matched", { n: String(matched) })}</Tag>{/if}
  </div>
  {#if result}
    <div class="tiles">
      <div class="tile"><small>{t("quality.tile.words")}</small><b class="num">{rateOf(result.before, "text")} → {rateOf(result.after, "text")}<em>%</em></b></div>
      <div class="tile"><small>{t("quality.tile.punct")}</small><b class="num">{rateOf(result.before, "punctuation")} → {rateOf(result.after, "punctuation")}<em>%</em></b></div>
      <div class="tile"><small>{t("quality.compare.words_tally")}</small><b>{@render tally(result.words)}</b></div>
      <div class="tile"><small>{t("quality.compare.punct_tally")}</small><b>{@render tally(result.punctuation)}</b></div>
    </div>
    {#if result.unmatched > 0}<p class="help">{t("quality.compare.unmatched", { n: String(result.unmatched) })}</p>{/if}
  {:else if before && !after}
    <p class="help">{t("quality.compare.same_stage")}</p>
  {/if}
</Group>

{#if result}
  {#if matched === 0}
    <Group><p class="help">{t("quality.compare.none")}</p></Group>
  {:else}
    <Group title={t("quality.by_category")}>
      <table>
        <thead><tr><th>{t("quality.col.category")}</th><th class="n">{t("quality.col.rows")}</th><th class="n">{t("quality.compare.col_words")}</th><th class="n">{t("quality.compare.col_punct")}</th></tr></thead>
        <tbody>
          {#each result.by_category as [name, w, p] (name)}
            <tr><td class="k">{categoryName(name)}</td><td class="n num">{w.improved + w.worse + w.same}</td><td class="n"><span class="pos num">{w.improved}</span> / <span class="neg num">{w.worse}</span></td><td class="n"><span class="pos num">{p.improved}</span> / <span class="neg num">{p.worse}</span></td></tr>
          {/each}
        </tbody>
      </table>
    </Group>

    <Group title={t("quality.compare.changed")}>
      {#if result.changed.length === 0}
        <p class="help">{t("quality.compare.no_change")}</p>
      {/if}
      {#each result.changed.slice(0, shown) as c, i (i)}
        <div class="item">
          <div class="h">
            <span class="num">#{c.id}</span>
            {#each c.categories as cat (cat)}<Tag>{categoryName(cat)}</Tag>{/each}
            <span class="sp"></span>
            <span class="num {tone(delta(c, 'words'))}">{t("quality.compare.row_words", { d: signed(delta(c, "words")) })}</span>
            <span class="num {tone(delta(c, 'punct'))}">{t("quality.compare.row_punct", { d: signed(delta(c, "punct")) })}</span>
          </div>
          <div class="line"><span class="lab">{t("quality.compare.before")}</span><TextDiff from={null} to={c.before_output} /></div>
          <div class="line"><span class="lab">{t("quality.compare.after")}</span><TextDiff from={c.before_output} to={c.after_output} /></div>
        </div>
      {/each}
      {#if result.changed.length > shown}
        <div class="foot"><button type="button" class="btn quiet" onclick={() => (shown = result!.changed.length)}>{t("quality.show_rest", { n: String(result.changed.length - shown) })}</button></div>
      {/if}
    </Group>
  {/if}
{/if}

<style>
  .head { display: flex; align-items: center; gap: 10px; padding: 12px var(--row-x); flex-wrap: wrap; }
  .head .field { width: 260px; }
  .head .vs { color: var(--ink-3); font-size: var(--text-xs); }
  .head .sp { flex: 1; }
  .tiles { display: grid; grid-template-columns: repeat(4, 1fr); border-top: 1px solid var(--line); }
  .tile { padding: 14px var(--row-x); border-left: 1px solid var(--line); min-width: 0; }
  .tile:first-child { border-left: 0; }
  .tile small { display: block; color: var(--ink-2); font-size: var(--text-sm); }
  .tile b { display: block; font: 600 var(--text-lg)/1.3 var(--font-round); margin-top: 4px; white-space: nowrap; }
  .tile b em { font-style: normal; font: 500 var(--text-sm)/1 var(--font); color: var(--ink-2); margin-left: 4px; }
  .pos { color: var(--ok); } .neg { color: var(--live); } .zero { color: var(--ink-3); }
  .help { margin: 0; padding: 10px var(--row-x) 14px; }
  table { width: 100%; border-collapse: collapse; font-size: var(--text-sm); }
  th { text-align: left; font-weight: 600; color: var(--ink-2); font-size: var(--text-xs); padding: 10px var(--row-x) 6px; }
  td { padding: 8px var(--row-x); border-top: 1px solid var(--line); }
  th.n, td.n { text-align: right; }
  td.k { font-weight: 500; }
  .item { padding: 12px var(--row-x); border-top: 1px solid var(--line); }
  .item:first-child { border-top: 0; }
  .h { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; font-size: var(--text-xs); color: var(--ink-2); margin-bottom: 6px; }
  .h .sp { flex: 1; }
  .line { display: flex; gap: 10px; align-items: baseline; }
  .line .lab { flex: none; width: 58px; color: var(--ink-3); font-size: var(--text-xs); }
  .line :global(.diff) { flex: 1; min-width: 0; }
  .foot { display: flex; justify-content: center; padding: 8px; border-top: 1px solid var(--line); }
</style>
