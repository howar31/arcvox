<script lang="ts" module>
  import type { Snippet as QSnippet } from "svelte";
  /** A question the dialog asks before it goes on: it takes the dialog's place until it is answered. */
  export interface Question {
    title: string;
    /** The answer "no": Esc and the backdrop give it, and the dialog's own content comes back. */
    no: () => void;
    body: QSnippet;
    actions: QSnippet;
  }
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import { tick } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fade, scale } from "svelte/transition";
  import { t } from "../i18n";
  import { isImeKey } from "../ime";

  let { open = $bindable(false), title, size = "md", grow = false, question = null, dirty = false, discardBody, children, actions, onclose, beforeclose }: {
    open?: boolean;
    title: string;
    /** "lg" is a wide dialog for a long form. */
    size?: "md" | "lg";
    /** For content that grows and shrinks while the dialog is open: the body lays out as a column, so a part of it can take the scrolling once the dialog reaches the window's height. */
    grow?: boolean;
    /** Shown instead of the content while set; the content keeps its state underneath. */
    question?: Question | null;
    /** The content differs from when the dialog opened: cancelling then asks before the changes are discarded. */
    dirty?: boolean;
    /** What the discard question says; a general sentence when unset. */
    discardBody?: string;
    children: Snippet;
    actions?: Snippet;
    /** Called when the user dismisses the dialog with Esc or the backdrop. */
    onclose?: () => void;
    /** Asked before Esc, the backdrop or `cancel()` closes the dialog; returning false keeps it open. */
    beforeclose?: () => boolean;
  } = $props();

  let box: HTMLDivElement | undefined = $state();
  let opener: HTMLElement | null = null;
  const titleId = `modal-${Math.random().toString(36).slice(2)}`;
  const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

  $effect(() => {
    if (!open) return;
    opener = document.activeElement as HTMLElement | null;
    tick().then(() => {
      // First tabbable element: skips roving-tabindex items such as unchecked segments.
      const first = [...(box?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [])].find((el) => el.tabIndex >= 0);
      (first ?? box)?.focus();
    });
    return () => {
      // Return focus to whatever opened the dialog.
      if (opener && document.contains(opener)) opener.focus();
      opener = null;
    };
  });

  /** Moves the dialog to the end of the document: a page lays itself out by its width (a size container),
   *  which would otherwise hold the fixed scrim to the page's area instead of the whole window. */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy: () => node.remove() };
  }

  // The question the dialog asks itself before changes are discarded.
  let discarding = $state(false);
  $effect(() => { if (!open) discarding = false; });
  function close() {
    discarding = false;
    open = false;
    onclose?.();
  }
  /** The press began on the backdrop itself: a drag that starts inside the dialog (selecting text) and ends outside is not a click on it. */
  let pressedScrim = false;
  /** Cancelling: Esc, the backdrop and the dialog's own cancel button all come here. Esc closes one layer: a question is answered no. */
  function dismiss() {
    if (question) { question.no(); return; }
    if (discarding) { discarding = false; return; }
    if (beforeclose && !beforeclose()) return;
    if (dirty) { discarding = true; return; }
    close();
  }
  /** The dialog's cancel button: the same as Esc. */
  export function cancel() {
    dismiss();
  }

  const tabbable = () => [...(box?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [])].filter((el) => el.tabIndex >= 0);

  function onkeydown(e: KeyboardEvent) {
    // Escape during composition cancels the input method, not the dialog.
    if (e.key === "Escape" && !isImeKey(e)) { e.preventDefault(); dismiss(); return; }
    if (e.key !== "Tab") return;
    const items = tabbable();
    if (!items.length) return;
    const first = items[0], last = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  }

  const asking = $derived<Question | null>(question ?? (discarding ? { title: t("common.discard_title"), no: () => (discarding = false), body: discardText, actions: discardActions } : null));

  // A question takes the focus to its first answer; the content gets it back afterwards.
  let beforeQuestion: HTMLElement | null = null;
  let asked = false;
  $effect(() => {
    const now = !!asking;
    if (now === asked) return;
    asked = now;
    if (now) {
      beforeQuestion = document.activeElement as HTMLElement | null;
      tick().then(() => box?.querySelector<HTMLElement>(".actions button:not([disabled])")?.focus());
    } else {
      const back = beforeQuestion;
      beforeQuestion = null;
      tick().then(() => { if (back && document.contains(back) && back.offsetParent !== null) back.focus(); else tabbable()[0]?.focus(); });
    }
  });

  /*
   * The dialog's height follows its content: a step, a question or a block that appears changes it at once,
   * so the body goes from the old height to the new one in a short animation, and the centred dialog moves
   * with it instead of jumping. Small changes and content that already slides (a fold) are left to move by themselves.
   */
  const STILL = () => matchMedia("(prefers-reduced-motion: reduce)").matches;
  let body: HTMLDivElement | undefined = $state();
  let lastHeight = 0;
  // While the body's own animation runs, a growing dialog's content follows it: those sizes are not news.
  let animating = false;
  // The watched content: set aside while the body animates, since a growing dialog's content follows the body then.
  const watched = new Set<HTMLElement>();
  const ro = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => resized());
  function sizeWatch(node: HTMLElement) {
    watched.add(node);
    if (!animating) ro?.observe(node);
    return { destroy: () => { watched.delete(node); ro?.unobserve(node); } };
  }
  function resized() {
    if (!body || animating) return;
    const next = body.getBoundingClientRect().height;
    const from = lastHeight;
    lastHeight = next;
    if (!from || Math.abs(next - from) < 48 || STILL()) return;
    const sliding = body.getAnimations({ subtree: true }).some((a) => a.effect?.getComputedTiming().endTime !== Infinity);
    if (sliding) return;
    animating = true;
    for (const n of watched) ro?.unobserve(n);
    const anim = body.animate([{ height: `${from}px` }, { height: `${next}px` }], { duration: 200, easing: "cubic-bezier(.22,.61,.36,1)" });
    anim.finished.catch(() => {}).finally(() => {
      animating = false;
      if (body) lastHeight = body.getBoundingClientRect().height;
      for (const n of watched) ro?.observe(n);
    });
  }
  const inTime = () => (STILL() ? 0 : 160);
  const outTime = () => (STILL() ? 0 : 120);

  /** The focused element went away with a step or a question: the keys stay with the dialog. */
  function onstray(e: KeyboardEvent) {
    if (!open || e.target !== document.body) return;
    if (e.key === "Tab") { e.preventDefault(); tabbable()[0]?.focus(); }
    else onkeydown(e);
  }
</script>

<svelte:document onkeydown={onstray} />

{#snippet discardText()}<p>{discardBody ?? t("common.discard_dialog_body")}</p>{/snippet}
{#snippet discardActions()}
  <button type="button" class="btn" onclick={() => (discarding = false)}>{t("common.cancel")}</button>
  <button type="button" class="btn danger" onclick={close}>{t("common.discard")}</button>
{/snippet}

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim" use:portal onpointerdown={(e) => { pressedScrim = e.target === e.currentTarget; }} onclick={(e) => { if (pressedScrim && e.target === e.currentTarget) dismiss(); pressedScrim = false; }} {onkeydown} in:fade|global={{ duration: inTime() }} out:fade|global={{ duration: outTime() }}>
    <div class="modal" class:lg={size === "lg"} class:grow role="dialog" aria-modal="true" aria-labelledby={titleId} tabindex="-1" bind:this={box} in:scale|global={{ start: 0.96, duration: inTime(), easing: cubicOut }} out:scale|global={{ start: 0.98, duration: outTime() }}>
      <h2 id={titleId}>{asking ? asking.title : title}</h2>
      <div class="body" bind:this={body}>
        <div class="inner" class:away={!!asking} use:sizeWatch>{@render children()}</div>
        {#if asking}<div class="inner" use:sizeWatch>{@render asking.body()}</div>{/if}
      </div>
      {#if asking}<div class="actions">{@render asking.actions()}</div>
      {:else if actions}<div class="actions">{@render actions()}</div>{/if}
    </div>
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; z-index: 50; display: grid; place-items: center; padding: 24px; background: var(--scrim); }
  /* The title and the actions stay in place; a long body scrolls between them. */
  .modal { width: min(540px, 100%); max-height: calc(100vh - 48px); display: flex; flex-direction: column; overflow: hidden; background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius-group); box-shadow: var(--shadow); }
  .modal:focus { outline: none; }
  h2 { flex: none; margin: 0; padding: 22px 22px 10px; font-size: var(--text-lg); font-weight: 600; }
  .body { flex: 0 1 auto; min-height: 0; overflow-y: auto; padding: 0 22px; color: var(--ink-2); }
  .body:last-child { padding-bottom: 18px; }
  .actions { flex: none; display: flex; justify-content: flex-end; gap: 10px; padding: 22px 22px 18px; }
  .lg { width: min(780px, 100%); }
  .modal.grow .body, .modal.grow .inner { display: flex; flex-direction: column; }
  .modal.grow .inner { flex: 0 1 auto; min-height: 0; }
  .modal .body > .inner.away { display: none; }
  .lg h2 { padding: 18px 22px 16px; }
  .lg .body { padding: 22px 22px 22px; color: var(--ink); }
  .lg .actions { padding: 14px 22px; }
</style>
