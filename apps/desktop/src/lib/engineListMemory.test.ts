import { beforeEach, describe, expect, it } from "vitest";
import { rememberFilter, rememberFold, storedFilter, storedFold } from "./engineListMemory";

// The tests run without a browser; a map stands in for the page's storage.
const store = new Map<string, string>();
beforeEach(() => {
  store.clear();
  (globalThis as { localStorage?: unknown }).localStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => { store.set(k, v); },
  };
});

describe("engine list memory", () => {
  it("shows every model until a filter is chosen, and ignores an unknown value", () => {
    expect(storedFilter()).toBe("all");
    rememberFilter("own");
    expect(storedFilter()).toBe("own");
    localStorage.setItem("arcvox.engines.filter", "bogus");
    expect(storedFilter()).toBe("all");
  });
  it("knows nothing of the fold until it is opened or shut", () => {
    expect(storedFold()).toBeNull();
    rememberFold(true);
    expect(storedFold()).toBe(true);
    rememberFold(false);
    expect(storedFold()).toBe(false);
  });
});
