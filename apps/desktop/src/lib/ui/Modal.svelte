<script lang="ts">
  import type { Snippet } from "svelte";
  import { tick } from "svelte";

  let { open = $bindable(false), title, children, actions, onclose }: {
    open?: boolean;
    title: string;
    children: Snippet;
    actions?: Snippet;
    /** Called when the user dismisses the dialog with Esc or the backdrop. */
    onclose?: () => void;
  } = $props();

  let box: HTMLDivElement | undefined = $state();
  let opener: HTMLElement | null = null;
  const titleId = `modal-${Math.random().toString(36).slice(2)}`;
  const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

  $effect(() => {
    if (!open) return;
    opener = document.activeElement as HTMLElement | null;
    tick().then(() => {
      const first = box?.querySelector<HTMLElement>(FOCUSABLE);
      (first ?? box)?.focus();
    });
    return () => {
      // Return focus to whatever opened the dialog.
      if (opener && document.contains(opener)) opener.focus();
      opener = null;
    };
  });

  function dismiss() {
    open = false;
    onclose?.();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); dismiss(); return; }
    if (e.key !== "Tab" || !box) return;
    const items = [...box.querySelectorAll<HTMLElement>(FOCUSABLE)];
    if (!items.length) return;
    const first = items[0], last = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim" onclick={(e) => { if (e.target === e.currentTarget) dismiss(); }} {onkeydown}>
    <div class="modal" role="dialog" aria-modal="true" aria-labelledby={titleId} tabindex="-1" bind:this={box}>
      <h2 id={titleId}>{title}</h2>
      <div class="body">{@render children()}</div>
      {#if actions}<div class="actions">{@render actions()}</div>{/if}
    </div>
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; z-index: 50; display: grid; place-items: center; padding: 24px; background: var(--scrim); }
  .modal { width: min(440px, 100%); max-height: calc(100vh - 48px); overflow: auto; background: var(--surface); border: 1px solid var(--line); border-radius: 12px; box-shadow: var(--shadow); padding: 20px 20px 16px; }
  .modal:focus { outline: none; }
  h2 { margin: 0 0 8px; font-size: 14px; font-weight: 600; }
  .body { color: var(--ink-2); }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 20px; }
</style>
