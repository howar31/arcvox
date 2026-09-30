import { beforeEach, expect, test } from "vitest";
import { toast } from "./toast.svelte";

beforeEach(() => {
  for (const item of toast.items) toast.dismiss(item.id);
});

test("an error already on screen is not shown twice", () => {
  toast.error("Could not save");
  toast.error("Could not save");
  expect(toast.items.map((i) => i.message)).toEqual(["Could not save"]);
});

test("different errors stack, and a dismissed error can show again", () => {
  toast.error("A");
  toast.error("B");
  expect(toast.items.map((i) => i.message)).toEqual(["A", "B"]);
  toast.dismiss(toast.items[0].id);
  toast.error("A");
  expect(toast.items.map((i) => i.message)).toEqual(["B", "A"]);
});
