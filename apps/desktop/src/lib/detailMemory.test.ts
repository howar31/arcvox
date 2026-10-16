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
  it("keeps every block closed until one is opened", () => {
    expect(storedOpen("stages")).toBe(false);
    expect(storedOpen("attempts")).toBe(false);
  });
  it("remembers each block on its own", () => {
    rememberOpen("attempts", true);
    expect(storedOpen("attempts")).toBe(true);
    expect(storedOpen("stages")).toBe(false);
    rememberOpen("attempts", false);
    expect(storedOpen("attempts")).toBe(false);
  });
});
