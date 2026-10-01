import { describe, expect, it } from "vitest";
import { messageDelay, TOTAL } from "./rewriteTiming";

describe("a message after the final text", () => {
  it("waits until the change has been shown and read", () => {
    expect(messageDelay("原文", "整理後", 1000, 1100, false)).toBe(1000 + TOTAL + 600 - 1100);
    expect(messageDelay("原文", "整理後", 1000, 1000 + TOTAL + 600, false)).toBe(0);
    expect(messageDelay("原文", "整理後", 1000, 9000, false)).toBe(0);
  });
  it("does not wait when nothing changes on the panel", () => {
    expect(messageDelay("原文", null, 0, 0, false)).toBe(0);
    expect(messageDelay("原文", "原文", 1000, 1100, false)).toBe(0);
    expect(messageDelay("", "整理後", 1000, 1100, false)).toBe(0);
    // Reduced motion: the final text is shown at once.
    expect(messageDelay("原文", "整理後", 1000, 1100, true)).toBe(0);
  });
});
