<script lang="ts">
  /** The text's way through a dictation, top to bottom: the transcript, each stage that changed it (with the change
   * marked), where it was put, and the user's correction read back from the field. */
  import type { Stage, Utterance } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import { textFlow } from "../../../lib/textFlow";
  import TextDiff from "../../../lib/ui/TextDiff.svelte";

  let { u, stages }: { u: Utterance; stages: Stage[] | null } = $props();
  const flow = $derived(textFlow(u, stages));
</script>

{#if flow.steps.length}
  <div class="label">{t("history.text_flow")}</div>
  <ol class="flow">
    {#each flow.steps as s, i (i)}
      <li class="step {s.kind}">
        <span class="dot">{#if s.kind === "user"}✓{:else if s.kind !== "mark"}{s.mark}{/if}</span>
        <div class="col">
          <div class="head"><b>{s.name}</b>{#if s.small}<span>{s.small}</span>{/if}</div>
          {#if s.kind === "first"}<div class="text first">{s.text}</div>
          {:else if s.kind === "failed"}<div class="text failed">{s.text}</div>
          {:else if s.kind === "stage" || s.kind === "user"}<div class="text"><TextDiff from={s.from} to={s.text} /></div>{/if}
        </div>
      </li>
    {/each}
  </ol>
  {#if flow.unchanged}<div class="unchanged">{flow.unchanged}</div>{/if}
{/if}

<style>
  .label { margin: 10px 0 5px; font-size: var(--text-xs); font-weight: 600; color: var(--ink-2); }
  :global(.detail) > .label:first-child { margin-top: 0; }
  .flow { position: relative; margin: 4px 0 0; padding: 0; list-style: none; -webkit-user-select: text; user-select: text; }
  .flow::before { content: ""; position: absolute; left: 9px; top: 12px; bottom: 12px; width: 2px; background: var(--line); }
  .step { position: relative; display: grid; grid-template-columns: 20px 1fr; gap: 0 12px; padding: 6px 0 8px; }
  .dot { position: relative; z-index: 1; display: grid; place-items: center; width: 20px; height: 20px; margin-top: 2px; border-radius: 50%; border: 2px solid var(--line-strong); background: var(--surface); color: var(--ink-2); font-size: 11px; font-weight: 700; }
  .user .dot { border-color: var(--accent); background: var(--accent); color: var(--accent-ink); }
  .mark .dot { width: 12px; height: 12px; margin: 6px 0 0 4px; background: var(--line-strong); border-color: var(--line-strong); }
  .col { min-width: 0; }
  .head { display: flex; align-items: baseline; gap: 8px; color: var(--ink-2); font-size: var(--text-sm); }
  .head b { color: var(--ink); font-weight: 600; }
  .user .head b { color: var(--accent); }
  .mark .head { padding-top: 1px; }
  .text { margin-top: 1px; line-height: 1.75; overflow-wrap: anywhere; white-space: pre-wrap; }
  .text.first { color: var(--ink-2); }
  .text.failed { color: var(--warn); font-size: var(--text-sm); }
  .unchanged { padding: 0 0 6px 32px; color: var(--ink-3); font-size: var(--text-xs); }
</style>
