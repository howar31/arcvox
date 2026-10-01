import { describe, expect, it } from "vitest";
import { setLanguage } from "./i18n";
import { defaultShortcuts, firstShortcut, isDefaultShortcuts, keyLabel, shortcutLabel } from "./shortcut";

describe("shortcut labels", () => {
  it("names modifiers with their side, in the interface language", () => {
    setLanguage("zh-TW");
    expect(keyLabel("MetaLeft")).toBe("左 ⌘");
    expect(keyLabel("AltRight")).toBe("右 ⌥");
    setLanguage("en");
    expect(keyLabel("ControlLeft")).toBe("Left ⌃");
    expect(keyLabel("ShiftRight")).toBe("Right ⇧");
  });
  it("names keys, numpad keys and mouse buttons", () => {
    setLanguage("zh-TW");
    expect(["Fn", "KeyD", "Digit5", "F13", "Comma", "ArrowLeft", "Backspace"].map(keyLabel)).toEqual(["fn", "D", "5", "F13", ",", "←", "⌫"]);
    expect(keyLabel("Numpad7")).toBe("數字鍵盤 7");
    expect(keyLabel("NumpadAdd")).toBe("數字鍵盤 +");
    expect(keyLabel("MouseMiddle")).toBe("滑鼠中鍵");
    expect(keyLabel("Mouse4")).toBe("滑鼠第 4 鍵");
  });
  it("joins the keys of a shortcut and falls back to fn", () => {
    setLanguage("en");
    expect(shortcutLabel(["MetaLeft", "KeyD"])).toBe("Left ⌘ + D");
    expect(firstShortcut([["AltRight"], ["Fn"]])).toBe("Right ⌥");
    expect(firstShortcut(undefined)).toBe("fn");
    expect(firstShortcut([])).toBe("fn");
  });
  it("knows the default list", () => {
    expect(isDefaultShortcuts(defaultShortcuts())).toBe(true);
    expect(isDefaultShortcuts([["Fn"], ["AltRight"]])).toBe(false);
    expect(isDefaultShortcuts([["AltRight"]])).toBe(false);
  });
});
