/**
 * The app has no context menus of its own, so the web view's default one is turned off.
 * Text fields keep the system menu (cut, copy, paste) unless `everywhere` is set.
 */
export function blockContextMenu(everywhere = false): void {
  document.addEventListener("contextmenu", (e) => {
    const field = "input:not([type=checkbox]):not([type=radio]), textarea";
    if (!everywhere && e.target instanceof Element && e.target.closest(field)) return;
    e.preventDefault();
  });
}
