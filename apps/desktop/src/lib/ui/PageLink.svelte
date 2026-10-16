<script lang="ts">
  /** The name of another page inside a sentence, as a link to it. */
  import { guard } from "../guard.svelte";
  import { go, type Page } from "../route";
  import { rememberStage } from "../stageMemory";
  import type { StageId } from "../flowLast";
  // `stage`: the stage the profiles page opens on, when the link leads there.
  let { page, arg = null, stage = null, label }: { page: Page; arg?: string | null; stage?: StageId | null; label: string } = $props();

  function open() {
    if (stage) rememberStage(stage);
    go(page, arg);
  }
</script>

<button type="button" class="page-link" onclick={() => guard.request(open)}>{label}</button>

<style>
  .page-link { border: 0; background: none; padding: 0; font: inherit; color: var(--accent); cursor: default; }
  .page-link:hover { color: var(--ink); }
</style>
