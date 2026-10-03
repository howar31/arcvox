<script lang="ts">
  import { onMount } from "svelte";
  import { errorMessage, getPrompt, restorePrompt, setPrompt } from "../../lib/api";
  import { guard } from "../../lib/guard.svelte";
  import { t } from "../../lib/i18n";
  import Modal from "../../lib/ui/Modal.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import SaveBar from "../../lib/ui/SaveBar.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  // The stored text; null until it is loaded.
  let saved = $state<string | null>(null);
  let text = $state("");
  let saving = $state(false);
  // False after a failed load: an editor that did not receive the stored text must not overwrite it.
  let loaded = $state(false);
  let confirmRestore = $state(false);

  onMount(async () => {
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
  <PageHead title={t("nav.prompt")} description={t("prompt.note")}>
    {#snippet actions()}
      <button type="button" class="btn" disabled={saving} onclick={() => (confirmRestore = true)}>{t("prompt.restore")}</button>
    {/snippet}
  </PageHead>

  <textarea class="editor" bind:value={text} aria-label={t("nav.prompt")} spellcheck="false"></textarea>

  <SaveBar {dirty} {saving} canSave={loaded} message={loaded ? undefined : t("prompt.not_loaded")} onsave={save} onrevert={() => (text = saved ?? "")} />

  <Modal bind:open={confirmRestore} title={t("prompt.restore_title")}>
    <p>{t("prompt.restore_body")}</p>
    {#snippet actions()}
      <button type="button" class="btn" onclick={() => (confirmRestore = false)}>{t("common.cancel")}</button>
      <button type="button" class="btn primary" onclick={restore}>{t("prompt.restore")}</button>
    {/snippet}
  </Modal>
{/if}

<style>
  /* Fills the window below the heading, and keeps a usable height in a short window. */
  .editor { display: block; height: max(220px, calc(100vh - 262px)); margin-top: 16px; }
</style>
