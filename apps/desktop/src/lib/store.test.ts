import { expect, test } from "vitest";
import { client, config } from "./store.svelte";

test("stores start empty", () => {
  expect(config.value).toBeNull();
  expect(config.error).toBeNull();
  expect(client.value).toBeNull();
});
