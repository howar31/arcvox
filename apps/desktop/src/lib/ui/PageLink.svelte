<script lang="ts">
  /** The name of another page inside a sentence, as a link to it. */
  import { guard } from "../guard.svelte";
  import { go, type Page } from "../route";
  let { page, arg = null, label }: { page: Page; arg?: string | null; label: string } = $props();
</script>

<button type="button" class="page-link" onclick={() => guard.request(() => go(page, arg))}>{label}</button>

<style>
  .page-link { border: 0; background: none; padding: 0; font: inherit; color: var(--accent); cursor: default; }
  .page-link:hover { color: var(--ink); }
</style>
