import { expect, test } from "vitest";
import { pageFromHash } from "./store.svelte";

test("hash routing falls back to general", () => {
  expect(pageFromHash("#history")).toBe("history");
  expect(pageFromHash("#nope")).toBe("general");
  expect(pageFromHash("")).toBe("general");
});
