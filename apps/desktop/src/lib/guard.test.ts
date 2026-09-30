import { beforeEach, expect, test } from "vitest";
import { guard } from "./guard.svelte";

beforeEach(() => {
  guard.cancel();
  guard.set(false);
});

test("a clean page runs the action at once", () => {
  let ran = 0;
  guard.request(() => ran++);
  expect(ran).toBe(1);
  expect(guard.pending).toBe(false);
});

test("a dirty page holds the action until confirmed", () => {
  let ran = 0;
  guard.set(true);
  guard.request(() => ran++);
  expect(ran).toBe(0);
  expect(guard.pending).toBe(true);
  guard.confirm();
  expect(ran).toBe(1);
  expect(guard.dirty).toBe(false);
  expect(guard.pending).toBe(false);
});

test("cancel drops the held action and keeps the changes", () => {
  let ran = 0;
  guard.set(true);
  guard.request(() => ran++);
  guard.cancel();
  expect(ran).toBe(0);
  expect(guard.pending).toBe(false);
  expect(guard.dirty).toBe(true);
  guard.confirm();
  expect(ran).toBe(0);
});
