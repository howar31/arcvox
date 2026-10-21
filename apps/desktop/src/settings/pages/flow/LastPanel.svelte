<script lang="ts">
  /** What a stage did in the profile's last dictation: a head line, and for the text stages the text with its changes marked. */
  import { formatBytes, type Stage, type Utterance } from "../../../lib/api";
  import { cellInput, cellLine, cellOutput, insertLines, promptLine, recordLines, stageAt, summaryLine, type CellId } from "../../../lib/flowLast";
  import { t } from "../../../lib/i18n";
  import Fold from "../../../lib/ui/Fold.svelte";
  import Icon from "../../../lib/ui/Icon.svelte";
  import TextDiff from "../../../lib/ui/TextDiff.svelte";

  let { id, stages, row = null }: { id: CellId; stages: Stage[] | null; row?: Utterance | null } = $props();

  // Each text stage's detail is folded until asked for; every stage keeps its own choice while the page is open.
  let opened = $state<Partial<Record<CellId, boolean>>>({});
  const open = $derived(opened[id] ?? false);

  const stage = $derived(stages ? stageAt(stages, id) : undefined);
  const textual = $derived(id === "asr" || id === "llm" || id === "rules" || id === "translate");
  const message = $derived(typeof stage?.detail.message === "string" ? stage.detail.message : typeof stage?.detail.error === "string" ? stage.detail.error : "");
  const language = $derived(typeof stage?.detail.language === "string" ? stage.detail.language : null);
  // The recognised text and what the conversion after it made of it; the same text twice when nothing was converted.
  const asrText = $derived(stages && id === "asr" ? stageAt(stages, "asr")?.text ?? null : null);
  const output = $derived(stages ? cellOutput(stages, id) : null);
  // The audio stage, the insertion and the record are told in the head line alone.
  const head = $derived.by(() => {
    if (!stages || !stage) return "";
    if (id === "insert") return insertLines(stages).join(t("flow.panel.sep"));
    if (id === "record") return recordLines(stages, row, formatBytes).join(t("flow.panel.sep"));
    return summaryLine(id, stages);
  });
</script>

<section class="panel" aria-label={t("flow.last")}>
  {#if !stages}
    <p class="none">{t("flow.last_none")}</p>
  {:else}
    {#if stage && textual}
      <button type="button" class="head fold" aria-expanded={open} onclick={() => (opened[id] = !open)}>
        <b>{t("flow.last")}</b><span>{head}</span><span class="chev" class:open><Icon name="chevron-down" size={14} /></span>
      </button>
    {:else}
      <div class="head"><b>{t("flow.last")}</b><span>{head}</span></div>
    {/if}
    {#if stage && textual && open}<Fold>
      <div class="body">
        {#if stage.text === null || output === null}
          <p class="failed">{t("flow.panel.failed", { message })}</p>
        {:else if id === "asr"}
          <TextDiff from={asrText !== null && asrText !== output ? asrText : null} to={output} />
          {#if language}<p class="meta">{t("flow.panel.language", { lang: language })}</p>{/if}
        {:else if id === "translate"}
          <p class="before">{cellInput(stages, id)}</p>
          <p class="after">{output}</p>
        {:else}
          <TextDiff from={cellInput(stages, id)} to={output} />
          {#if id === "llm"}<p class="meta">{promptLine(stage)}</p>{/if}
        {/if}
      </div>
    </Fold>{:else if !stage}
      <p class="none">{cellLine(id, stages)}</p>
    {/if}
  {/if}
</section>

<style>
  .panel { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); }
  .head { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; width: 100%; padding: 12px 18px; font-size: var(--text-sm); color: var(--ink-2); }
  .head b { color: var(--ink); font-weight: 600; font-size: var(--text-control); flex: none; }
  .head span { text-align: right; }
  .head span:not(.chev) { flex: 1; min-width: 0; }
  .fold { border: 0; background: none; text-align: left; cursor: default; border-radius: var(--radius-group); }
  .fold:hover { background: color-mix(in srgb, var(--sunken) 60%, transparent); }
  .fold[aria-expanded="true"] { border-radius: var(--radius-group) var(--radius-group) 0 0; }
  .chev { flex: none; align-self: center; display: inline-flex; color: var(--ink-3); transition: transform .15s; }
  .chev.open { transform: rotate(180deg); }
  .body { padding: 12px 18px 14px; border-top: 1px solid var(--line); }
  .meta { margin: 8px 0 0; color: var(--ink-3); font-size: var(--text-xs); line-height: 1.5; }
  .failed { margin: 0; color: var(--warn); font-size: var(--text-sm); }
  .none { margin: 0; padding: 14px 18px; color: var(--ink-3); font-size: var(--text-sm); }
  /* A translation is shown whole, the text before it above in the quieter colour: a character diff across languages says nothing. */
  .before { margin: 0 0 8px; color: var(--ink-2); line-height: 1.7; white-space: pre-wrap; overflow-wrap: anywhere; }
  .after { margin: 0; line-height: 1.7; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
