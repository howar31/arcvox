import { t } from "./i18n";

const MODIFIERS: Record<string, string> = { Meta: "⌘", Alt: "⌥", Control: "⌃", Shift: "⇧" };
const NAMED: Record<string, string> = {
  Fn: "fn", Space: "Space", Escape: "Esc", Enter: "↩", Tab: "⇥", Backspace: "⌫", Delete: "⌦",
  ArrowLeft: "←", ArrowRight: "→", ArrowUp: "↑", ArrowDown: "↓", Home: "Home", End: "End", PageUp: "Page Up", PageDown: "Page Down",
  Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]", Backslash: "\\", Semicolon: ";", Quote: "'", Backquote: "`",
  Comma: ",", Period: ".", Slash: "/",
};
const NUMPAD: Record<string, string> = { Decimal: ".", Add: "+", Subtract: "-", Multiply: "*", Divide: "/", Equal: "=", Enter: "↩" };

/** What a key or mouse button is called in the interface. `code` is the name the app stores. */
export function keyLabel(code: string): string {
  const side = /^(Meta|Alt|Control|Shift)(Left|Right)$/.exec(code);
  if (side) return t(side[2] === "Left" ? "shortcut.left" : "shortcut.right", { key: MODIFIERS[side[1]] });
  if (code in NAMED) return NAMED[code];
  if (code === "MouseMiddle") return t("shortcut.mouse_middle");
  if (code.startsWith("Mouse")) return t("shortcut.mouse_n", { n: code.slice(5) });
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (code.startsWith("Numpad")) return t("shortcut.numpad", { key: NUMPAD[code.slice(6)] ?? code.slice(6) });
  return code;
}

/** A shortcut in words, its keys joined with a plus sign. */
export function shortcutLabel(keys: string[]): string {
  return keys.map(keyLabel).join(" + ");
}

/** The shortcut shown where one is named in a sentence: the first one set. */
export function firstShortcut(shortcuts: string[][] | undefined): string {
  return shortcutLabel(shortcuts?.[0] ?? ["Fn"]);
}

/** The shortcuts after the first one, as a list in words; empty when there is only one. */
export function otherShortcuts(shortcuts: string[][] | undefined): string {
  return (shortcuts ?? []).slice(1).map(shortcutLabel).join(", ");
}

/** The shortcuts of a new installation: the fn key alone. */
export function defaultShortcuts(): string[][] {
  return [["Fn"]];
}

export function isDefaultShortcuts(shortcuts: string[][]): boolean {
  return shortcuts.length === 1 && shortcuts[0].length === 1 && shortcuts[0][0] === "Fn";
}
