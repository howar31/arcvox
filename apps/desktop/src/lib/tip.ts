// The one tooltip of the settings window: a card under the element the pointer rests on. Every hover
// explanation (a tag's meaning, a lamp's colour, an icon button's name, a cut-off value, the ⓘ's second
// layer) uses the same card, the same timing and the same place, so nothing in the window looks like a
// different kind of hint. The widget and the panel are separate small windows and keep the system's.

import { placeTip } from "./popover";

/** How long the pointer rests before the card appears. */
export const SHOW_MS = 500;
/** How long after the pointer leaves the card goes, so it can be reached across the gap. */
export const HIDE_MS = 150;
/** Within this time after a card went, the next one appears at once. */
export const RESHOW_MS = 500;

/** The card type by its text: a short label stays on one line, a longer one wraps. */
export function tipKind(text: string): "short" | "long" {
  return text.length <= 40 && !text.includes("\n") ? "short" : "long";
}

let closeCurrent: (() => void) | null = null;
let lastClosed = 0;

/** The delay before the next card: none right after one went. */
export function showDelay(): number {
  return Date.now() - lastClosed < RESHOW_MS ? 0 : SHOW_MS;
}

/** Registers the card that is open now; the previous one is closed first. */
export function opened(close: () => void): void {
  if (closeCurrent && closeCurrent !== close) closeCurrent();
  closeCurrent = close;
}

/** Records that the card registered with `opened` has gone. */
export function closed(close: () => void): void {
  if (closeCurrent === close) closeCurrent = null;
  lastClosed = Date.now();
}

/** Puts `card` under `anchor`, inside the viewport (above when short of room below). */
export function placeCard(card: HTMLElement, anchor: HTMLElement): void {
  const r = anchor.getBoundingClientRect();
  const p = placeTip(
    { top: r.top, bottom: r.bottom, left: r.left, width: r.width },
    { width: card.offsetWidth, height: card.offsetHeight },
    { width: window.innerWidth, height: window.innerHeight },
  );
  card.style.left = `${p.left}px`;
  card.style.top = p.top !== null ? `${p.top}px` : "";
  card.style.bottom = p.bottom !== null ? `${p.bottom}px` : "";
}

/** One card element for every `tip` action; it is made on first use and moved as needed. */
let shared: HTMLDivElement | null = null;
function card(): HTMLDivElement {
  if (!shared) {
    shared = document.createElement("div");
    shared.className = "tipcard";
    shared.setAttribute("role", "tooltip");
    document.body.appendChild(shared);
  }
  return shared;
}

/**
 * `use:tip={text}`: the card shows `text` while the pointer rests on the element (or it has keyboard
 * focus). When `text` is the element's own text, the card shows only when that text is cut off.
 */
export function tip(node: HTMLElement, text: string | undefined | null) {
  let current = text ?? "";
  let showTimer: ReturnType<typeof setTimeout> | undefined;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  let open = false;

  const close = () => {
    clearTimeout(showTimer);
    clearTimeout(hideTimer);
    if (!open) return;
    open = false;
    const c = card();
    c.style.visibility = "hidden";
    closed(close);
  };

  const wanted = () => {
    if (!current) return false;
    // The full value of a cut-off text: nothing to add while it all shows.
    if (current === node.textContent?.trim()) return node.scrollWidth > node.clientWidth + 1;
    return true;
  };

  const show = () => {
    if (open || !wanted()) return;
    opened(close);
    open = true;
    const c = card();
    c.textContent = current;
    c.classList.toggle("short", tipKind(current) === "short");
    c.style.visibility = "hidden";
    c.style.display = "block";
    placeCard(c, node);
    c.style.visibility = "visible";
  };

  const enter = () => {
    clearTimeout(hideTimer);
    if (open) return;
    clearTimeout(showTimer);
    showTimer = setTimeout(show, showDelay());
  };
  const leave = () => {
    clearTimeout(showTimer);
    hideTimer = setTimeout(close, HIDE_MS);
  };
  const focus = () => {
    if (node.matches(":focus-visible")) show();
  };
  const onScroll = () => close();

  node.addEventListener("pointerenter", enter);
  node.addEventListener("pointerleave", leave);
  node.addEventListener("pointerdown", close);
  node.addEventListener("focus", focus);
  node.addEventListener("blur", close);
  document.addEventListener("scroll", onScroll, true);

  return {
    update(next: string | undefined | null) {
      current = next ?? "";
      if (open) {
        if (!current) close();
        else {
          const c = card();
          c.textContent = current;
          placeCard(c, node);
        }
      }
    },
    destroy() {
      close();
      node.removeEventListener("pointerenter", enter);
      node.removeEventListener("pointerleave", leave);
      node.removeEventListener("pointerdown", close);
      node.removeEventListener("focus", focus);
      node.removeEventListener("blur", close);
      document.removeEventListener("scroll", onScroll, true);
    },
  };
}
