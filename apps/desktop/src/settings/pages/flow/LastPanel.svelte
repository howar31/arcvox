<script lang="ts">
  /** What a stage did in the profile's last dictation: a head line, and for the text stages the text with its changes marked. */
  import type { Stage } from "../../../lib/api";
  import { cellLine, inputOf, promptLine, stageAt, summaryLine, type StageId } from "../../../lib/flowLast";
  import { t } from "../../../lib/i18n";
  import TextDiff from "../../../lib/ui/TextDiff.svelte";

  let { id, stages }: { id: StageId; stages: Stage[] | null } = $props();

  const stage = $derived(stages ? stageAt(stages, id) : undefined);
  const textual = $derived(id === "asr" || id === "opencc1" || id === "llm" || id === "opencc2" || id === "rules");
  const message = $derived(typeof stage?.detail.message === "string" ? stage.detail.message : typeof stage?.detail.error === "string" ? stage.detail.error : "");
  const language = $derived(typeof stage?.detail.language === "string" ? stage.detail.language : null);
</script>

<section class="panel" aria-label={t("flow.last")}>
  {#if !stages}
    <p class="none">{t("flow.last_none")}</p>
  {:else}
    <div class="head"><b>{t("flow.last")}</b><span>{summaryLine(id, stages)}</span></div>
    {#if stage && textual}
      <div class="body">
        {#if stage.text === null}
          <p class="failed">{t("flow.panel.failed", { message })}</p>
        {:else if id === "asr"}
          <TextDiff from={null} to={stage.text} />
          {#if language}<p class="meta">{t("flow.panel.language", { lang: language })}</p>{/if}
        {:else}
          <TextDiff from={inputOf(stages, id)} to={stage.text} />
          {#if id === "llm"}<p class="meta">{promptLine(stage)}</p>{/if}
        {/if}
      </div>
    {:else if !stage}
      <p class="none">{cellLine(id, stages)}</p>
    {/if}
  {/if}
</section>

<style>
  .panel { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); }
  .head { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; padding: 12px 18px; font-size: var(--text-sm); color: var(--ink-2); }
  .head b { color: var(--ink); font-weight: 600; font-size: var(--text-control); flex: none; }
  .head span { text-align: right; }
  .body { padding: 12px 18px 14px; border-top: 1px solid var(--line); }
  .meta { margin: 8px 0 0; color: var(--ink-3); font-size: var(--text-xs); line-height: 1.5; }
  .failed { margin: 0; color: var(--warn); font-size: var(--text-sm); }
  .none { margin: 0; padding: 14px 18px; color: var(--ink-3); font-size: var(--text-sm); }
</style>
