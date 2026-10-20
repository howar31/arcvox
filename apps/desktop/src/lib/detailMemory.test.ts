import { beforeEach, describe, expect, it } from "vitest";
import { rememberOpen, storedOpen } from "./detailMemory";

// The tests run without a browser; a map stands in for the page's storage.
const store = new Map<string, string>();
beforeEach(() => {
  store.clear();
  (globalThis as { localStorage?: unknown }).localStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => { store.set(k, v); },
  };
});

describe("detail memory", () => {
  it("keeps a block closed until it is opened, and remembers the state", () => {
    expect(storedOpen("attempts")).toBe(false);
    rememberOpen("attempts", true);
    expect(storedOpen("attempts")).toBe(true);
    rememberOpen("attempts", false);
    expect(storedOpen("attempts")).toBe(false);
  });
});
