import { beforeEach, describe, expect, it } from "vitest";
import { rememberStage, storedStage } from "./stageMemory";

// The tests run without a browser; a map stands in for the page's storage.
const store = new Map<string, string>();
beforeEach(() => {
  store.clear();
  (globalThis as { localStorage?: unknown }).localStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => { store.set(k, v); },
  };
});

describe("stage memory", () => {
  it("starts at the speech stage when nothing is remembered", () => {
    expect(storedStage()).toBe("asr");
  });
  it("gives back the remembered stage", () => {
    rememberStage("rules");
    expect(storedStage()).toBe("rules");
  });
  it("ignores a value that is not a stage", () => {
    localStorage.setItem("arcvox.flow.stage", "bogus");
    expect(storedStage()).toBe("asr");
  });
});
