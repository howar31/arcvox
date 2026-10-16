import { describe, expect, it } from "vitest";
import { formatBytes } from "./api";

describe("formatBytes", () => {
  it("uses gigabytes with two decimals from 1e9", () => {
    expect(formatBytes(1080732091)).toBe("1.08 GB");
  });
  it("uses whole megabytes below that", () => {
    expect(formatBytes(574041195)).toBe("574 MB");
  });
  it("uses megabytes with one decimal below ten, and kilobytes below one", () => {
    expect(formatBytes(1_840_000)).toBe("1.8 MB");
    expect(formatBytes(192_044)).toBe("192 KB");
    expect(formatBytes(0)).toBe("0 KB");
  });
});

import { rowStatus } from "./models.svelte";
import type { ModelEntry } from "./api";
const caps = { transcribe: true, chat: false, streaming: false, language_hint: true, hotwords: true };
const entry: ModelEntry = { id: "a", runtime: "whisper", role: "asr", label: "A", description: "", languages: [], url: "https://h/a.bin", sha256: "0".repeat(64), size_bytes: 10, license: "MIT", recommended: true, min_ram_gb: 8 };

describe("rowStatus", () => {
  it("is missing when nothing is installed or running", () => {
    expect(rowStatus(entry, [], {})).toEqual({ kind: "missing" });
  });
  it("is downloading while progress is not done", () => {
    expect(rowStatus(entry, [], { a: { id: "a", received: 4, total: 10, done: false, error: null } })).toEqual({ kind: "downloading", received: 4, total: 10 });
  });
  it("is installed when the file exists", () => {
    expect(rowStatus(entry, [{ id: "a", runtime: "whisper", path: "/p", size_bytes: 10, custom: false, capabilities: caps, encoder: null, facts: { shape: null, quantization: null, license: null } }], {})).toEqual({ kind: "installed", size: 10, custom: false });
  });
  it("shows the error from a finished failed download", () => {
    expect(rowStatus(entry, [], { a: { id: "a", received: 0, total: 10, done: true, error: "checksum mismatch" } })).toEqual({ kind: "error", message: "checksum mismatch" });
  });
  it("installed wins over a stale error", () => {
    expect(rowStatus(entry, [{ id: "a", runtime: "whisper", path: "/p", size_bytes: 10, custom: false, capabilities: caps, encoder: null, facts: { shape: null, quantization: null, license: null } }], { a: { id: "a", received: 0, total: 10, done: true, error: "x" } }).kind).toBe("installed");
  });
});
