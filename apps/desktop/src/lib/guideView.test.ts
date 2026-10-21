import { expect, test } from "vitest";
import type { InstalledModel, ManifestView, ModelEntry } from "./api";
import { applePath, bytesToDownload, downloadTotals, engineStepDone, guideEntries, memoryShort, permissionStepDone } from "./guideView";

const entry = (id: string, role: "asr" | "llm", size_bytes: number, extra: Partial<ModelEntry> = {}): ModelEntry =>
  ({ id, runtime: "llama", role, label: id, description: "", languages: [], url: "", sha256: "", size_bytes, license: "", recommended: false, min_ram_gb: 8, ...extra }) as ModelEntry;

const manifest: ManifestView = {
  source: "bundled",
  models: [
    entry("whisper-large-v3-q5_0", "asr", 1),
    entry("qwen3-asr-1.7b-q8_0", "asr", 2_000, { mmproj: { url: "", sha256: "", size_bytes: 300 } }),
    entry("breeze-asr-25-q5_0", "asr", 1_000),
    entry("qwen3-8b-q4_k_m", "llm", 5_000, { min_ram_gb: 16 }),
    entry("gemma-3-4b-it-q4_k_m", "llm", 2_500),
  ],
};
const installed = (...ids: string[]): InstalledModel[] => ids.map((id) => ({ id }) as InstalledModel);

test("the guide offers its two models per role in a fixed order, the rest behind the fold", () => {
  expect(guideEntries(manifest, "asr").map((m) => m.id)).toEqual(["breeze-asr-25-q5_0", "qwen3-asr-1.7b-q8_0"]);
  expect(guideEntries(manifest, "asr", true).map((m) => m.id)).toEqual(["breeze-asr-25-q5_0", "qwen3-asr-1.7b-q8_0", "whisper-large-v3-q5_0"]);
  expect(guideEntries(manifest, "llm").map((m) => m.id)).toEqual(["gemma-3-4b-it-q4_k_m", "qwen3-8b-q4_k_m"]);
  expect(guideEntries(null, "llm")).toEqual([]);
});

test("sizes count the audio encoder and leave out what is installed", () => {
  const picked = [manifest.models[1], manifest.models[4]];
  expect(bytesToDownload(picked, [])).toBe(2_300 + 2_500);
  expect(bytesToDownload(picked, installed("gemma-3-4b-it-q4_k_m"))).toBe(2_300);
  expect(downloadTotals(picked, installed("gemma-3-4b-it-q4_k_m"), { "qwen3-asr-1.7b-q8_0": { id: "qwen3-asr-1.7b-q8_0", received: 1_000, total: 2_300, done: false, error: null } }))
    .toEqual({ received: 3_500, total: 4_800 });
});

test("a model that asks for more memory than the machine has is marked, unknown memory is not", () => {
  expect(memoryShort(manifest.models[3], 8 * 1024 ** 3)).toBe(true);
  expect(memoryShort(manifest.models[3], 16 * 1024 ** 3)).toBe(false);
  expect(memoryShort(manifest.models[3], 0)).toBe(false);
});

test("the Apple path follows the Foundation Models status", () => {
  expect(applePath("ready")).toBe("ready");
  expect(applePath("intelligence_off")).toBe("off");
  expect(applePath("model_not_ready")).toBe("preparing");
  expect(applePath("not_eligible")).toBe("none");
  expect(applePath("os_too_old")).toBe("none");
  expect(applePath(null)).toBe("none");
});

test("the engine step goes on once a download started, the Apple services are ready, or the user leaves it for later", () => {
  expect(engineStepDone("models", "none", false, 100)).toBe(false);
  expect(engineStepDone("models", "none", true, 100)).toBe(true);
  expect(engineStepDone("models", "none", false, 0)).toBe(true);
  expect(engineStepDone("apple", "off", false, 100)).toBe(false);
  expect(engineStepDone("apple", "ready", false, 100)).toBe(true);
  expect(engineStepDone("later", "none", false, 100)).toBe(true);
});

test("the permission step needs Accessibility and a microphone the user can still do something about", () => {
  expect(permissionStepDone(null)).toBe(false);
  expect(permissionStepDone({ accessibility: false, keyboard_watch_only: false, microphone: "granted" })).toBe(false);
  expect(permissionStepDone({ accessibility: true, keyboard_watch_only: false, microphone: "not_asked" })).toBe(false);
  expect(permissionStepDone({ accessibility: true, keyboard_watch_only: false, microphone: "denied" })).toBe(false);
  expect(permissionStepDone({ accessibility: true, keyboard_watch_only: false, microphone: "granted" })).toBe(true);
  expect(permissionStepDone({ accessibility: true, keyboard_watch_only: false, microphone: "restricted" })).toBe(true);
});
