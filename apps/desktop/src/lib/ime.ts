/** Whether a key press belongs to an input method rather than the page: pressed while
 * composing, or (as WebKit sends it) the Enter that confirms a candidate right after
 * composition ended, which carries keyCode 229. Handlers for Enter, comma or Escape in
 * text fields ignore these. */
export function isImeKey(e: KeyboardEvent): boolean {
  return e.isComposing || e.keyCode === 229;
}
