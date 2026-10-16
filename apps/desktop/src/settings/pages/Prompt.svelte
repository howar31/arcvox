<script lang="ts">
  import { onMount } from "svelte";
  import { defaultPrompt, errorMessage, getPrompt, restorePrompt, setPrompt } from "../../lib/api";
  import { guard } from "../../lib/guard.svelte";
  import { lineDiff } from "../../lib/lineDiff";
  import { t } from "../../lib/i18n";
  import Modal from "../../lib/ui/Modal.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import PageLink from "../../lib/ui/PageLink.svelte";
  import SaveBar from "../../lib/ui/SaveBar.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  // The stored text; null until it is loaded.
  let saved = $state<string | null>(null);
  let text = $state("");
  let saving = $state(false);
  // False after a failed load: an editor that did not receive the stored text must not overwrite it.
  let loaded = $state(false);
  let confirmRestore = $state(false);
  // The text of a new installation; null while it is not known.
  let standard = $state<string | null>(null);
  // Restoring is offered while the stored text may differ from it.
  const restorable = $derived(!loaded || standard === null || saved !== standard);

  onMount(async () => {
    defaultPrompt().then((d) => { standard = d; }).catch(() => {});
    try {
      const current = await getPrompt();
      saved = current;
      text = current;
      loaded = true;
    } catch (e) {
      // Still show the editor, so the default can be restored.
      saved = "";
      text = "";
      toast.error(errorMessage(e));
    }
  });

  const dirty = $derived(saved !== null && text !== saved);

  $effect(() => {
    guard.set(dirty);
    return () => guard.set(false);
  });

  async function save() {
    if (!loaded) return;
    saving = true;
    try {
      const next = text;
      await setPrompt(next);
      saved = next;
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      saving = false;
    }
  }

  async function restore() {
    confirmRestore = false;
    saving = true;
    try {
      const next = await restorePrompt();
      saved = next;
      text = next;
      loaded = true;
      toast.ok(t("prompt.restored"));
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      saving = false;
    }
  }
</script>

{#if saved !== null}
  {#snippet noteInfo()}{t("prompt.note_info_before").slice(0, -1)}<span class="nobr">{t("prompt.note_info_before").slice(-1)}<PageLink page="flow" stage="rules" label={t("nav.flow")} />{t("prompt.note_info_after")}</span>{/snippet}
  <PageHead title={t("nav.prompt")} description={t("prompt.note")} info={noteInfo} crumb={{ label: t("nav.flow"), page: "flow" }}>
    {#snippet actions()}
      {#if restorable}<button type="button" class="btn" disabled={saving} onclick={() => (confirmRestore = true)}>{t("prompt.restore")}</button>{/if}
    {/snippet}
  </PageHead>

  <textarea class="editor" bind:value={text} aria-label={t("nav.prompt")} spellcheck="false"></textarea>

  <SaveBar {dirty} {saving} canSave={loaded} message={loaded ? undefined : t("prompt.not_loaded")} onsave={save} onrevert={() => (text = saved ?? "")} />

  <Modal bind:open={confirmRestore} title={t("prompt.restore_title")} size="lg">
    <p>{t("prompt.restore_body")}</p>
    {#if standard === null}
      <p class="muted">{t("prompt.default_unknown")}</p>
    {:else}
      <p class="legend"><span class="sw del"></span>{t("prompt.diff_current")}<span class="sw ins"></span>{t("prompt.diff_default")}</p>
      <div class="lines">
        {#each lineDiff(text, standard) as l, i (i)}
          <div class="ln {l.kind}">{l.text || " "}</div>
        {/each}
      </div>
    {/if}
    {#snippet actions()}
      <button type="button" class="btn" onclick={() => (confirmRestore = false)}>{t("common.cancel")}</button>
      <button type="button" class="btn primary" onclick={restore}>{t("prompt.restore")}</button>
    {/snippet}
  </Modal>
{/if}

<style>
  /* Fills the window below the heading, and keeps a usable height in a short window. */
  .editor { display: block; height: max(220px, calc(100vh - 262px)); margin-top: 16px; }
  .muted { color: var(--ink-2); }
  .legend { display: flex; align-items: center; gap: 8px; margin: 0 0 8px; font-size: var(--text-xs); color: var(--ink-2); }
  .sw { display: inline-block; width: 12px; height: 12px; border-radius: 3px; }
  .sw.del { background: color-mix(in srgb, var(--live) 24%, transparent); }
  .sw.ins { background: color-mix(in srgb, var(--ok) 26%, transparent); }
  .lines { max-height: 50vh; overflow: auto; border: 1px solid var(--line); border-radius: var(--radius); padding: 8px 0; font-size: var(--text-sm); line-height: 1.6; }
  .ln { padding: 0 12px; white-space: pre-wrap; overflow-wrap: anywhere; }
  .ln.del { background: color-mix(in srgb, var(--live) 12%, transparent); color: var(--ink-2); text-decoration: line-through; text-decoration-color: color-mix(in srgb, var(--live) 60%, transparent); }
  .ln.ins { background: color-mix(in srgb, var(--ok) 14%, transparent); }
</style>
