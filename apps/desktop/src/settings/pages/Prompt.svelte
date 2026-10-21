<script lang="ts">
  import { defaultPrompt, defaultTranslatePrompt, errorMessage, fixedPrompts, getPrompt, getTranslatePrompt, restorePrompt, restoreTranslatePrompt, setPrompt, setTranslatePrompt } from "../../lib/api";
  import { guard } from "../../lib/guard.svelte";
  import { stageName } from "../../lib/flowLast";
  import { lineDiff } from "../../lib/lineDiff";
  import { t } from "../../lib/i18n";
  import Modal from "../../lib/ui/Modal.svelte";
  import PageHead from "../../lib/ui/PageHead.svelte";
  import PageLink from "../../lib/ui/PageLink.svelte";
  import SaveBar from "../../lib/ui/SaveBar.svelte";
  import { toast } from "../../lib/ui/toast.svelte";

  /** `arg` names the module whose prompt is shown: `llm` (the default) or `translate`. */
  let { arg }: { arg: string | null } = $props();
  type Kind = "rewrite" | "translate";
  const kind = $derived<Kind>(arg === "translate" ? "translate" : "rewrite");
  const api = {
    rewrite: { get: getPrompt, set: setPrompt, standard: defaultPrompt, restore: restorePrompt },
    translate: { get: getTranslatePrompt, set: setTranslatePrompt, standard: defaultTranslatePrompt, restore: restoreTranslatePrompt },
  };

  /** One prompt's editor: the stored rules, the draft, and the text of a new installation. */
  interface Block {
    // The stored text; null until it is loaded.
    saved: string | null;
    text: string;
    // False after a failed load: an editor that did not receive the stored text must not overwrite it.
    loaded: boolean;
    // The text of a new installation; null while it is not known.
    standard: string | null;
  }
  let b = $state<Block>({ saved: null, text: "", loaded: false, standard: null });
  // The opening the app sends before the rules; shown, never edited.
  let fixed = $state<string | null>(null);
  let saving = $state(false);
  let confirmRestore = $state(false);

  // Another module's prompt: the editor starts over with its text.
  $effect(() => {
    const k = kind;
    b = { saved: null, text: "", loaded: false, standard: null };
    fixed = null;
    load(k);
  });

  async function load(k: Kind) {
    fixedPrompts().then((f) => { if (k === kind) fixed = f[k]; }).catch(() => {});
    api[k].standard().then((d) => { if (k === kind) b.standard = d; }).catch(() => {});
    try {
      const current = await api[k].get();
      if (k !== kind) return;
      b.saved = current;
      b.text = current;
      b.loaded = true;
    } catch (e) {
      if (k !== kind) return;
      // Still show the editor, so the default can be restored.
      b.saved = "";
      b.text = "";
      toast.error(errorMessage(e));
    }
  }

  const dirty = $derived(b.saved !== null && b.text !== b.saved);
  // Restoring is offered while the stored text may differ from the default.
  const restorable = $derived(!b.loaded || b.standard === null || b.saved !== b.standard);

  $effect(() => {
    guard.set(dirty);
    return () => guard.set(false);
  });

  async function save() {
    if (!b.loaded) return;
    saving = true;
    try {
      const next = b.text;
      await api[kind].set(next);
      b.saved = next;
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
      const next = await api[kind].restore();
      b.saved = next;
      b.text = next;
      b.loaded = true;
      toast.ok(t("prompt.restored"));
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      saving = false;
    }
  }

  const stage = $derived(stageName(kind === "rewrite" ? "llm" : "translate"));
</script>

{#if b.saved !== null}
  {#snippet noteInfo()}{t("prompt.note_info_before").slice(0, -1)}<span class="nobr">{t("prompt.note_info_before").slice(-1)}<PageLink page="flow" stage="rules" label={t("nav.flow")} />{t("prompt.note_info_after")}</span>{/snippet}
  <PageHead title={t("prompt.title", { stage })} description={t(kind === "rewrite" ? "prompt.rewrite_note" : "prompt.translate_note")} info={noteInfo} crumb={{ label: t("nav.flow"), page: "flow" }}>
    {#snippet actions()}
      {#if restorable}<button type="button" class="btn" disabled={saving} onclick={() => (confirmRestore = true)}>{t("prompt.restore")}</button>{/if}
    {/snippet}
  </PageHead>

  {#if fixed !== null}
    <div class="fixed" aria-label={t("prompt.fixed")}>
      <span class="fixed-label">{t("prompt.fixed")}</span>
      <pre>{fixed}</pre>
    </div>
  {/if}
  <textarea class="editor" bind:value={b.text} aria-label="{stage} {t('prompt.rules')}" spellcheck="false"></textarea>

  <SaveBar {dirty} {saving} canSave={b.loaded} message={b.loaded ? undefined : t("prompt.not_loaded")} onsave={save} onrevert={() => (b.text = b.saved ?? "")} />

  <Modal bind:open={confirmRestore} title={t("prompt.restore_title")} size="lg">
    <p>{t("prompt.restore_body")}</p>
    {#if b.standard === null}
      <p class="muted">{t("prompt.default_unknown")}</p>
    {:else}
      <p class="legend"><span class="sw del"></span>{t("prompt.diff_current")}<span class="sw ins"></span>{t("prompt.diff_default")}</p>
      <div class="lines">
        {#each lineDiff(b.text, b.standard) as l, i (i)}
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
  /* The opening the app sends first: read like the editor, but plainly not a field. */
  .fixed { margin: 16px 0 8px; padding: 10px 13px; border-radius: 10px; background: var(--sunken); color: var(--ink-2); font-size: var(--text-sm); line-height: 1.6; }
  .fixed-label { display: block; margin-bottom: 4px; font-size: var(--text-xs); color: var(--ink-3); }
  .fixed pre { margin: 0; font: inherit; white-space: pre-wrap; overflow-wrap: anywhere; }
  /* Fills the window below the opening, and keeps a usable height in a short window. */
  .editor { display: block; height: max(180px, calc(100vh - 420px)); }
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
