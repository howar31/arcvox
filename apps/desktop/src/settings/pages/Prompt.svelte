<script lang="ts">
  import { onMount } from "svelte";
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

  type Kind = "rewrite" | "translate";
  const KINDS: Kind[] = ["rewrite", "translate"];
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
  const blank = (): Block => ({ saved: null, text: "", loaded: false, standard: null });
  let blocks = $state<Record<Kind, Block>>({ rewrite: blank(), translate: blank() });
  // The openings the app sends before the rules; shown, never edited.
  let fixed = $state<Record<Kind, string> | null>(null);
  let saving = $state(false);
  let confirmRestore = $state<Kind | null>(null);

  onMount(async () => {
    fixedPrompts().then((f) => { fixed = f; }).catch(() => {});
    for (const kind of KINDS) {
      const b = blocks[kind];
      api[kind].standard().then((d) => { b.standard = d; }).catch(() => {});
      try {
        const current = await api[kind].get();
        b.saved = current;
        b.text = current;
        b.loaded = true;
      } catch (e) {
        // Still show the editor, so the default can be restored.
        b.saved = "";
        b.text = "";
        toast.error(errorMessage(e));
      }
    }
  });

  const shown = $derived(KINDS.every((k) => blocks[k].saved !== null));
  const dirtyOf = (b: Block) => b.saved !== null && b.text !== b.saved;
  const dirty = $derived(KINDS.some((k) => dirtyOf(blocks[k])));
  // Saving needs every changed editor to hold the stored text.
  const canSave = $derived(KINDS.every((k) => !dirtyOf(blocks[k]) || blocks[k].loaded));
  // Restoring is offered while the stored text may differ from the default.
  const restorable = (b: Block) => !b.loaded || b.standard === null || b.saved !== b.standard;

  $effect(() => {
    guard.set(dirty);
    return () => guard.set(false);
  });

  async function save() {
    if (!canSave) return;
    saving = true;
    try {
      for (const kind of KINDS) {
        const b = blocks[kind];
        if (!dirtyOf(b)) continue;
        const next = b.text;
        await api[kind].set(next);
        b.saved = next;
      }
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      saving = false;
    }
  }

  function revert() {
    for (const kind of KINDS) blocks[kind].text = blocks[kind].saved ?? "";
  }

  async function restore(kind: Kind) {
    confirmRestore = null;
    saving = true;
    try {
      const next = await api[kind].restore();
      const b = blocks[kind];
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

  const title = (kind: Kind) => stageName(kind === "rewrite" ? "llm" : "translate");
</script>

{#if shown}
  {#snippet noteInfo()}{t("prompt.note_info_before").slice(0, -1)}<span class="nobr">{t("prompt.note_info_before").slice(-1)}<PageLink page="flow" stage="rules" label={t("nav.flow")} />{t("prompt.note_info_after")}</span>{/snippet}
  <PageHead title={t("nav.prompt")} description={t("prompt.note")} info={noteInfo} crumb={{ label: t("nav.flow"), page: "flow" }} />

  {#each KINDS as kind (kind)}
    {@const b = blocks[kind]}
    <section class="block" data-section={kind}>
      <div class="bar">
        <h2>{title(kind)}</h2>
        {#if restorable(b)}<button type="button" class="btn" disabled={saving} onclick={() => (confirmRestore = kind)}>{t("prompt.restore")}</button>{/if}
      </div>
      <p class="sub">{t(kind === "rewrite" ? "prompt.rewrite_note" : "prompt.translate_note")}</p>
      {#if fixed}
        <div class="fixed" aria-label={t("prompt.fixed")}>
          <span class="fixed-label">{t("prompt.fixed")}</span>
          <pre>{fixed[kind]}</pre>
        </div>
      {/if}
      <textarea class="editor" bind:value={b.text} aria-label="{title(kind)} {t('prompt.rules')}" spellcheck="false"></textarea>
    </section>
  {/each}

  <SaveBar {dirty} {saving} {canSave} message={canSave ? undefined : t("prompt.not_loaded")} onsave={save} onrevert={revert} />

  <Modal open={confirmRestore !== null} onclose={() => (confirmRestore = null)} title={t("prompt.restore_title")} size="lg">
    {#if confirmRestore}
      {@const b = blocks[confirmRestore]}
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
    {/if}
    {#snippet actions()}
      <button type="button" class="btn" onclick={() => (confirmRestore = null)}>{t("common.cancel")}</button>
      <button type="button" class="btn primary" onclick={() => confirmRestore && restore(confirmRestore)}>{t("prompt.restore")}</button>
    {/snippet}
  </Modal>
{/if}

<style>
  .block { margin-top: var(--gap-group); scroll-margin-top: 20px; }
  .bar { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin: 0 4px 6px; min-height: 28px; }
  .bar h2 { margin: 0; font: 600 var(--text)/1.2 var(--font); }
  .sub { display: block; margin: 0 4px 10px; }
  /* The opening the app sends first: read like the editor, but plainly not a field. */
  .fixed { margin: 0 0 8px; padding: 10px 13px; border-radius: 10px; background: var(--sunken); color: var(--ink-2); font-size: var(--text-sm); line-height: 1.6; }
  .fixed-label { display: block; margin-bottom: 4px; font-size: var(--text-xs); color: var(--ink-3); }
  .fixed pre { margin: 0; font: inherit; white-space: pre-wrap; overflow-wrap: anywhere; }
  .editor { display: block; min-height: 180px; }
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
