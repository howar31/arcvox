import { expect, test } from "vitest";
import type { EngineCheck, LocalService, ProviderConfig, ServicePreset } from "./api";
import { blankHttp } from "./engines";
import { checkBody, checkCurrent, checkKey, cloudFor, localFor, localRows, modesOf, pickOf, servesOther, suggestedIn, takesEffort } from "./serviceForm";

const svc = (id: string, kind: "cloud" | "local", asr: ServicePreset["asr"], llm: ServicePreset["llm"]): ServicePreset => ({
  id, label: id, kind, base_url: kind === "cloud" ? `https://${id}.test/v1` : null, port: kind === "local" ? 1 : null,
  key_required: kind === "cloud", asr, llm, suggested: {},
});
const SERVICES = [svc("gemini", "cloud", "chat_audio", "chat"), svc("openai", "cloud", "transcriptions", "chat"), svc("ollama", "local", "transcriptions", "chat"), svc("lm-studio", "local", null, "chat"), svc("speaches", "local", "transcriptions", null)];

test("each tab lists the services that do its use", () => {
  expect(cloudFor(SERVICES, "asr").map((s) => s.id)).toEqual(["gemini", "openai"]);
  expect(localFor(SERVICES, "asr").map((s) => s.id)).toEqual(["ollama", "speaches"]);
  expect(localFor(SERVICES, "llm").map((s) => s.id)).toEqual(["ollama", "lm-studio"]);
});

test("the other use is offered when the service does it, and always for an address typed by hand", () => {
  expect(servesOther({ kind: "cloud", id: "gemini" }, SERVICES, "asr")).toBe(true);
  expect(servesOther({ kind: "local", id: "speaches" }, SERVICES, "asr")).toBe(false);
  expect(servesOther({ kind: "local", id: null }, SERVICES, "asr")).toBe(false);
  expect(servesOther({ kind: "custom" }, SERVICES, "asr")).toBe(true);
});

test("a reasoning effort goes with chat requests only", () => {
  expect(takesEffort({ kind: "cloud", id: "gemini" }, SERVICES, "asr")).toBe(true);
  expect(takesEffort({ kind: "cloud", id: "openai" }, SERVICES, "asr")).toBe(false);
  expect(takesEffort({ kind: "custom" }, SERVICES, "asr")).toBe(false);
  expect(takesEffort({ kind: "custom" }, SERVICES, "llm")).toBe(true);
  const routed = SERVICES.map((s) => (s.id === "gemini" ? { ...s, asr_routes: [{ models: ["t"], wire: "interactions" as const, base_url: "https://g.test/", modes: ["verbatim", "smart"] }] } : s));
  expect(takesEffort({ kind: "cloud", id: "gemini" }, routed, "asr", "t")).toBe(false);
  expect(takesEffort({ kind: "cloud", id: "gemini" }, routed, "asr", "flash")).toBe(true);
  expect(modesOf({ kind: "cloud", id: "gemini" }, routed, "t")).toEqual(["verbatim", "smart"]);
  expect(modesOf({ kind: "cloud", id: "gemini" }, routed, "flash")).toEqual([]);
  expect(modesOf({ kind: "custom" }, routed, "t")).toEqual([]);
});

test("an engine opens on the pick it was made with", () => {
  const p = (service: string | null): ProviderConfig => ({ ...blankHttp("asr"), service });
  expect(pickOf(p("gemini"), SERVICES)).toEqual({ kind: "cloud", id: "gemini" });
  expect(pickOf(p("ollama"), SERVICES)).toEqual({ kind: "local", id: "ollama" });
  expect(pickOf(p(null), SERVICES)).toEqual({ kind: "custom" });
  expect(pickOf(p("gone"), SERVICES)).toEqual({ kind: "custom" });
});

test("a suggestion matches a listed model with or without its prefix", () => {
  expect(suggestedIn(["models/gemini-2.5-flash", "x"], "gemini-2.5-flash")).toBe("models/gemini-2.5-flash");
  expect(suggestedIn(["gemini-2.5-flash"], "gemini-2.5-flash")).toBe("gemini-2.5-flash");
  expect(suggestedIn(["a"], "b")).toBeNull();
  expect(suggestedIn(["a"], null)).toBeNull();
});

test("a check is current only for the service, model and effort it was made with", () => {
  const p: ProviderConfig = { ...blankHttp("asr"), service: "gemini", base_url: "https://g.test/", model: "m", reasoning_effort: null };
  const c: EngineCheck = { ok: true, at: 1, elapsed_ms: 1, role: "asr", service: "gemini", base_url: "https://old.test/", model: "m", reasoning_effort: null };
  // A service engine's address comes from the catalogue, so it does not count.
  expect(checkCurrent(c, p, "asr")).toBe(true);
  expect(checkCurrent(c, { ...p, model: "n" }, "asr")).toBe(false);
  expect(checkCurrent(c, { ...p, reasoning_effort: "low" }, "asr")).toBe(false);
  expect(checkCurrent(c, p, "llm")).toBe(false);
  expect(checkCurrent(undefined, p, "asr")).toBe(false);
  const custom = { ...p, service: null, base_url: "http://h.test/v1" };
  expect(checkKey(custom, "asr")).not.toBe(checkKey({ ...custom, base_url: "http://other.test/v1" }, "asr"));
});

test("local rows list the tab's services, running ones first", () => {
  const found: LocalService[] = [
    { id: "ollama", label: "Ollama", base_url: "http://127.0.0.1:11434/v1", running: false, models: [] },
    { id: "lm-studio", label: "LM Studio", base_url: "http://127.0.0.1:1234/v1", running: true, models: [] },
    { id: "speaches", label: "Speaches", base_url: "http://127.0.0.1:8000/v1", running: true, models: [] },
  ];
  expect(localRows(found, SERVICES, "asr").map((f) => f.id)).toEqual(["speaches", "ollama"]);
});

test("an empty reply reads the same whatever the service said", () => {
  const base = { ok: false, at: 1, elapsed_ms: 2, role: "asr" as const };
  expect(checkBody({ ...base, code: "empty_reply", message: "Gemini: x" })).not.toBe("Gemini: x");
  expect(checkBody({ ...base, code: "provider_error", message: "Gemini: x" })).toBe("Gemini: x");
});
