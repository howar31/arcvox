<script lang="ts">
  import type { Snippet } from "svelte";
  import { tick } from "svelte";
  import { isImeKey } from "../ime";

  let { open = $bindable(false), title, size = "md", children, actions, onclose, beforeclose }: {
    open?: boolean;
    title: string;
    /** "lg" is a wide dialog for a long form. */
    size?: "md" | "lg";
    children: Snippet;
    actions?: Snippet;
    /** Called when the user dismisses the dialog with Esc or the backdrop. */
    onclose?: () => void;
    /** Asked before Esc or the backdrop closes the dialog; returning false keeps it open. */
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

  function dismiss() {
    if (beforeclose && !beforeclose()) return;
    open = false;
    onclose?.();
  }

  function onkeydown(e: KeyboardEvent) {
    // Escape during composition cancels the input method, not the dialog.
    if (e.key === "Escape" && !isImeKey(e)) { e.preventDefault(); dismiss(); return; }
    if (e.key !== "Tab" || !box) return;
    const items = [...box.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => el.tabIndex >= 0);
    if (!items.length) return;
    const first = items[0], last = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim" onclick={(e) => { if (e.target === e.currentTarget) dismiss(); }} {onkeydown}>
    <div class="modal" class:lg={size === "lg"} role="dialog" aria-modal="true" aria-labelledby={titleId} tabindex="-1" bind:this={box}>
      <h2 id={titleId}>{title}</h2>
      <div class="body">{@render children()}</div>
      {#if actions}<div class="actions">{@render actions()}</div>{/if}
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
  .lg h2 { padding: 18px 22px 16px; }
  .lg .body { padding: 22px 22px 22px; color: var(--ink); }
  .lg .actions { padding: 14px 22px; }
</style>
