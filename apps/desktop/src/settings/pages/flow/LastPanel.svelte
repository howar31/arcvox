<script lang="ts">
  /** What a text stage did in the profile's last dictation: the text with its changes marked, and what the stage was given. */
  import type { Stage } from "../../../lib/api";
  import { cellInput, cellOutput, promptLine, sentLine, stageAt, type CellId } from "../../../lib/flowLast";
  import { t } from "../../../lib/i18n";
  import TextDiff from "../../../lib/ui/TextDiff.svelte";

  let { id, stages }: { id: CellId; stages: Stage[] } = $props();

  const stage = $derived(stageAt(stages, id));
  const message = $derived(typeof stage?.detail.message === "string" ? stage.detail.message : typeof stage?.detail.error === "string" ? stage.detail.error : "");
  const language = $derived(typeof stage?.detail.language === "string" ? stage.detail.language : null);
  // The recognised text and what the conversion after it made of it; the same text twice when nothing was converted.
  const asrText = $derived(id === "asr" ? stageAt(stages, "asr")?.text ?? null : null);
  const output = $derived(cellOutput(stages, id));
  const meta = $derived([
    sentLine(id, stages),
    id === "asr" && language ? t("flow.panel.language", { lang: language }) : "",
    id === "llm" && stage && stage.text !== null ? promptLine(stage) : "",
  ].filter(Boolean).join(t("flow.panel.sep")));
</script>

<div class="body" role="region" aria-label={t("flow.last")}>
  {#if !stage || stage.text === null || output === null}
    <p class="failed">{t("flow.panel.failed", { message })}</p>
  {:else if id === "asr"}
    <TextDiff from={asrText !== null && asrText !== output ? asrText : null} to={output} />
  {:else if id === "translate"}
    <p class="before">{cellInput(stages, id)}</p>
    <p class="after">{output}</p>
  {:else}
    <TextDiff from={cellInput(stages, id)} to={output} />
  {/if}
  {#if meta}<p class="meta">{meta}</p>{/if}
</div>

<style>
  .body { margin: 0 16px; padding: 12px 6px 14px; }
  .meta { margin: 8px 0 0; color: var(--ink-3); font-size: var(--text-xs); line-height: 1.5; }
  .failed { margin: 0; color: var(--warn); font-size: var(--text-sm); }
  /* A translation is shown whole, the text before it above in the quieter colour: a character diff across languages says nothing. */
  .before { margin: 0 0 8px; color: var(--ink-2); line-height: 1.7; white-space: pre-wrap; overflow-wrap: anywhere; }
  .after { margin: 0; line-height: 1.7; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
