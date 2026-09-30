import { expect, it } from "vitest";
import { isImeKey } from "./ime";

const key = (init: { key: string; isComposing?: boolean; keyCode?: number }) =>
  ({ key: init.key, isComposing: init.isComposing ?? false, keyCode: init.keyCode ?? 13 }) as KeyboardEvent;

it("treats a key pressed during composition as input method input", () => {
  expect(isImeKey(key({ key: "Enter", isComposing: true }))).toBe(true);
});

it("treats the Enter that confirms a candidate after composition ended as input method input", () => {
  expect(isImeKey(key({ key: "Enter", keyCode: 229 }))).toBe(true);
});

it("lets a plain Enter through", () => {
  expect(isImeKey(key({ key: "Enter" }))).toBe(false);
  expect(isImeKey(key({ key: ",", keyCode: 188 }))).toBe(false);
});
