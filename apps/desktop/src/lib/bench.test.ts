import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BenchRun } from "./api";

const api = vi.hoisted(() => ({
  benchStart: vi.fn(async () => {}),
  benchCancel: vi.fn(async () => {}),
  onBenchProgress: vi.fn(async () => () => {}),
  onBenchDone: vi.fn(async () => () => {}),
}));
vi.mock("./api", () => api);

import { bench } from "./bench.svelte";

const run = (id: string, cold: boolean): BenchRun => ({ provider_id: id, role: "asr", cold, load_ms: cold ? 900 : 0, infer_ms: 1200, output: "text", error: null });

describe("bench", () => {
  beforeEach(() => {
    bench.done();
    bench.runs = [];
    api.benchStart.mockClear();
  });

  it("is running from the start until the run is reported done, with two results expected per engine", async () => {
    await bench.start(["a"], ["b", "c"], "sentence");
    expect(api.benchStart).toHaveBeenCalledWith(["a"], ["b", "c"], "sentence");
    expect(bench.running).toBe(true);
    expect(bench.remaining).toBe(6);
    bench.progress(run("a", true), 5);
    bench.progress(run("a", false), 4);
    expect(bench.runs.map((r) => r.cold)).toEqual([true, false]);
    expect(bench.remaining).toBe(4);
    bench.done();
    expect(bench.running).toBe(false);
    expect(bench.remaining).toBe(0);
    expect(bench.runs).toHaveLength(2);
  });

  it("drops the previous results when a new run starts", async () => {
    bench.progress(run("a", true), 0);
    await bench.start(["a"], [], "sentence");
    expect(bench.runs).toEqual([]);
  });

  it("is not running when the run could not start", async () => {
    api.benchStart.mockRejectedValueOnce({ message: "no test clip" });
    await expect(bench.start(["a"], [], "sentence")).rejects.toEqual({ message: "no test clip" });
    expect(bench.running).toBe(false);
  });

  it("listens for progress once, however often the page is opened", () => {
    bench.watch();
    bench.watch();
    expect(api.onBenchProgress).toHaveBeenCalledTimes(1);
    expect(api.onBenchDone).toHaveBeenCalledTimes(1);
  });
});
