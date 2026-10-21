<script lang="ts">
  /** The text's way through a dictation, top to bottom: the transcript, each stage that changed it (with the change
   * marked), where it was put, and the user's correction, read back from the field or typed here. */
  import type { Stage, Utterance } from "../../../lib/api";
  import { t } from "../../../lib/i18n";
  import { textFlow } from "../../../lib/textFlow";
  import TextDiff from "../../../lib/ui/TextDiff.svelte";

  let { u, stages, oncorrect }: {
    u: Utterance;
    stages: Stage[] | null;
    /** Keeps `text` as the row's final text; false when it could not be kept. Without it the flow is read-only. */
    oncorrect?: (text: string) => Promise<boolean>;
  } = $props();
  const flow = $derived(textFlow(u, stages));
  const hasCorrection = $derived(flow.steps.some((s) => s.kind === "user"));
  // A row with a result can take a correction: a failed or cancelled one has no text to correct.
  const canCorrect = $derived(!!oncorrect && u.error === null && u.refined_text.trim() !== "");

  let editing = $state(false);
  let draft = $state("");
  let saving = $state(false);
  // The remove button was pressed once; a second press within a few seconds removes the correction.
  let sure = $state(false);
  let sureTimer: ReturnType<typeof setTimeout> | undefined;

  function startEdit() {
    draft = u.final_text;
    editing = true;
  }
  async function save() {
    if (!oncorrect || saving) return;
    saving = true;
    const ok = await oncorrect(draft.trim());
    saving = false;
    if (ok) editing = false;
  }
  function askRemove() {
    clearTimeout(sureTimer);
    sure = true;
    sureTimer = setTimeout(() => { sure = false; }, 4000);
  }
  async function remove() {
    clearTimeout(sureTimer);
    sure = false;
    if (oncorrect) await oncorrect(u.refined_text.trim());
  }
  function grow(el: HTMLTextAreaElement, _value: string) {
    const fit = () => {
      el.style.height = "auto";
      el.style.height = `${el.scrollHeight + el.offsetHeight - el.clientHeight}px`;
    };
    fit();
    el.addEventListener("input", fit);
    el.focus();
    el.setSelectionRange(el.value.length, el.value.length);
    return { update: fit, destroy: () => el.removeEventListener("input", fit) };
  }
</script>

{#if flow.steps.length}
  <div class="label">{t("history.text_flow")}</div>
  <ol class="flow">
    {#each flow.steps as s, i (i)}
      <li class="step {s.kind}">
        <span class="dot">{#if s.kind === "user"}✓{:else if s.kind !== "mark"}{s.mark}{/if}</span>
        <div class="col">
          <div class="head">
            <b>{s.name}</b>{#if s.small}<span>{s.small}</span>{/if}
            {#if s.kind === "user" && canCorrect && !editing}
              <span class="acts">
                <button type="button" class="btn quiet" onclick={startEdit}>{t("history.correct_edit")}</button>
                {#if sure}<button type="button" class="btn danger" onclick={remove}>{t("history.correct_remove_sure")}</button>
                {:else}<button type="button" class="btn quiet danger" onclick={askRemove}>{t("history.correct_remove")}</button>{/if}
              </span>
            {/if}
          </div>
          {#if s.kind === "first"}<div class="text first">{s.text}</div>
          {:else if s.kind === "failed"}<div class="text failed">{s.text}</div>
          {:else if s.kind === "user" && editing}
            <div class="ed">
              <textarea rows="2" bind:value={draft} use:grow={draft} aria-label={t("history.flow_correction")}></textarea>
              <div class="btns">
                <button type="button" class="btn primary" disabled={saving || !draft.trim()} onclick={save}>{t("common.save")}</button>
                <button type="button" class="btn quiet" onclick={() => (editing = false)}>{t("common.cancel")}</button>
                <span class="hint">{t("history.correct_hint")}</span>
              </div>
            </div>
          {:else if s.kind === "stage" || s.kind === "user"}<div class="text"><TextDiff from={s.from} to={s.text} /></div>{/if}
        </div>
      </li>
    {/each}
    {#if canCorrect && !hasCorrection}
      <li class="step user add">
        <span class="dot">✓</span>
        <div class="col">
          {#if editing}
            <div class="head"><b>{t("history.flow_correction")}</b></div>
            <div class="ed">
              <textarea rows="2" bind:value={draft} use:grow={draft} aria-label={t("history.flow_correction")}></textarea>
              <div class="btns">
                <button type="button" class="btn primary" disabled={saving || !draft.trim()} onclick={save}>{t("common.save")}</button>
                <button type="button" class="btn quiet" onclick={() => (editing = false)}>{t("common.cancel")}</button>
                <span class="hint">{t("history.correct_hint")}</span>
              </div>
            </div>
          {:else}
            <div class="head"><button type="button" class="btn quiet add-btn" onclick={startEdit}>{t("history.correct_add")}</button></div>
          {/if}
        </div>
      </li>
    {/if}
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
  /* A stage that changed nothing: a dashed ring and quiet words, one line, no text under it. */
  .same { padding: 4px 0 5px; }
  .same .dot { border-style: dashed; color: var(--ink-3); }
  .same .head { padding-top: 2px; }
  .same .head b { color: var(--ink-3); font-weight: 500; }
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
  .head .acts { display: inline-flex; gap: 4px; margin-left: auto; }
  .head .acts .btn, .add-btn { padding: 1px 8px; font-size: var(--text-sm); }
  .add.user .dot { background: var(--surface); color: var(--line-strong); border-style: dashed; }
  .add .head { padding-top: 0; }
  .ed { margin-top: 4px; }
  .ed textarea { display: block; width: 100%; box-sizing: border-box; min-height: 44px; padding: 8px 10px; font: inherit; line-height: 1.6; color: var(--ink); background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); resize: none; }
  .ed textarea:focus { outline: 2px solid var(--accent); outline-offset: -1px; }
  .ed .btns { display: flex; align-items: center; gap: 8px; margin-top: 8px; }
  .ed .hint { color: var(--ink-3); font-size: var(--text-xs); }
</style>
