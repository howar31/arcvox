import { describe, expect, it } from "vitest";
import { formatBytes } from "./api";

describe("formatBytes", () => {
  it("uses gigabytes with two decimals from 1e9", () => {
    expect(formatBytes(1080732091)).toBe("1.08 GB");
  });
  it("uses whole megabytes below that", () => {
    expect(formatBytes(574041195)).toBe("574 MB");
  });
});
