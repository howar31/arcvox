<script lang="ts">
  /** The stages of a dictation that changed its text, each with the change marked; a failed stage says so. */
  import type { Stage } from "../../../lib/api";
  import { cellLine, changedStages, inputOf, stageAt, stageName } from "../../../lib/flowLast";
  import { t } from "../../../lib/i18n";
  import TextDiff from "../../../lib/ui/TextDiff.svelte";

  let { stages }: { stages: Stage[] } = $props();
  const ids = $derived(changedStages(stages));
</script>

{#if ids.length === 0}
  <div class="none">{t("history.stages_none")}</div>
{:else}
  <div class="list">
    {#each ids as id (id)}
      {@const s = stageAt(stages, id)!}
      <div class="stage">
        <div class="head"><span class="num">{s.position}</span><b>{stageName(id)}</b><span>{cellLine(id, stages)}</span></div>
        {#if s.text === null}
          <!-- The head already says the step failed; the body carries the engine's reason. -->
          <div class="failed">{typeof s.detail.message === "string" ? s.detail.message : typeof s.detail.error === "string" ? s.detail.error : ""}</div>
        {:else}
          <TextDiff from={inputOf(stages, id)} to={s.text} />
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .none { color: var(--ink-2); font-size: var(--text-sm); }
  .failed { color: var(--warn); font-size: var(--text-sm); }
  .list { display: flex; flex-direction: column; }
  .stage { padding: 10px 0 11px; -webkit-user-select: text; user-select: text; }
  .stage + .stage { border-top: 1px solid var(--line); }
  .head { display: flex; align-items: center; gap: 8px; margin-bottom: 4px; color: var(--ink-2); font-size: var(--text-sm); }
  .head b { color: var(--ink); font-weight: 600; }
  .num { display: inline-grid; place-items: center; width: 20px; height: 20px; border-radius: 50%; background: var(--sunken); color: var(--ink-2); font-size: var(--text-xs); font-weight: 600; }
</style>
