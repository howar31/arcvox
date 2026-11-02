import type { EngineCheck, LocalService, ProviderConfig, ServicePreset, Wire } from "./api";
import type { Role } from "./engines";
import { t } from "./i18n";

/** What the form's first step picked: a catalogue service, a local one found running, or an address typed by hand. */
export type ServicePick = { kind: "cloud"; id: string } | { kind: "local"; id: string | null } | { kind: "custom" };

/** The service the pick names; null for an address typed by hand or no local service yet. */
export const pickId = (p: ServicePick): string | null => (p.kind === "custom" ? null : p.id);

export const otherRole = (r: Role): Role => (r === "asr" ? "llm" : "asr");

/** How the role's requests go to the service; null when it does not do that. */
export const wireOf = (s: ServicePreset | undefined, role: Role): Wire | null => (s ? (s[role] ?? null) : null);

/** The cloud services that do the role, in catalogue order. */
export const cloudFor = (services: ServicePreset[], role: Role): ServicePreset[] =>
  services.filter((s) => s.kind === "cloud" && wireOf(s, role) !== null);

/** The local services that do the role. */
export const localFor = (services: ServicePreset[], role: Role): ServicePreset[] =>
  services.filter((s) => s.kind === "local" && wireOf(s, role) !== null);

/** Whether an engine added through the pick can also be made for the other role. An address typed by hand may do anything. */
export function servesOther(pick: ServicePick, services: ServicePreset[], role: Role): boolean {
  if (pick.kind === "custom") return true;
  if (!pick.id) return false;
  return wireOf(services.find((s) => s.id === pick.id), otherRole(role)) !== null;
}

/** The way a speech model of the service goes when it has one of its own. */
const modelRoute = (s: ServicePreset | undefined, model: string) => [...(s?.asr_routes ?? []), ...(s?.live_routes ?? [])].find((r) => r.models.includes(model));

/** The modes the speech model offers, the default first; empty when it has none. */
export function modesOf(pick: ServicePick, services: ServicePreset[], model: string): string[] {
  if (pick.kind === "custom" || !pick.id) return [];
  return modelRoute(services.find((s) => s.id === pick.id), model)?.modes ?? [];
}

/** Whether the speech model can say when each word was said. */
export function offersWordTimes(pick: ServicePick, services: ServicePreset[], model: string): boolean {
  if (pick.kind === "custom" || !pick.id) return false;
  return !!modelRoute(services.find((s) => s.id === pick.id), model)?.word_times;
}

/** Whether an engine asks for word times: they exist in the service's default mode only. */
export const asksWordTimes = (p: Pick<ProviderConfig, "word_times" | "transcribe_mode">): boolean => !!p.word_times && !p.transcribe_mode;

/** Whether the role's requests are chat requests, which take a reasoning effort. */
export function takesEffort(pick: ServicePick, services: ServicePreset[], role: Role, model = ""): boolean {
  if (role === "llm") return true;
  if (pick.kind === "custom" || !pick.id) return false;
  const s = services.find((x) => x.id === pick.id);
  return wireOf(s, role) === "chat_audio" && !modelRoute(s, model);
}

/** The pick an existing engine was made with. */
export function pickOf(p: ProviderConfig | null, services: ServicePreset[]): ServicePick {
  const s = p?.service ? services.find((x) => x.id === p.service) : undefined;
  if (s) return s.kind === "cloud" ? { kind: "cloud", id: s.id } : { kind: "local", id: s.id };
  if (p) return { kind: "custom" };
  const first = services.find((x) => x.kind === "cloud");
  return first ? { kind: "cloud", id: first.id } : { kind: "custom" };
}

/** A model the service lists that matches the suggestion, with or without a `models/` prefix. */
export function suggestedIn(listed: string[], suggestion: string | null | undefined): string | null {
  if (!suggestion) return null;
  return listed.find((m) => m === suggestion || m.endsWith(`/${suggestion}`)) ?? null;
}

/** What a real request depends on: a change to any of these makes an earlier check stale. */
export const checkKey = (p: Pick<ProviderConfig, "service" | "base_url" | "model" | "reasoning_effort" | "transcribe_mode" | "word_times">, role: Role): string =>
  JSON.stringify([role, p.service ?? null, p.service ? null : (p.base_url ?? null), p.model ?? null, p.reasoning_effort ?? null, p.transcribe_mode ?? null, !!p.word_times]);

/** Whether a stored check was made with the engine as it is set now. */
export const checkCurrent = (c: EngineCheck | undefined, p: ProviderConfig, role: Role): boolean =>
  !!c && checkKey({ service: c.service ?? null, base_url: c.base_url ?? null, model: c.model ?? null, reasoning_effort: c.reasoning_effort ?? null, transcribe_mode: c.transcribe_mode ?? null, word_times: c.word_times }, c.role) === checkKey(p, role);

/** What a failed check shows as the service's words. */
export const checkBody = (c: EngineCheck): string | null | undefined => (c.code === "empty_reply" ? t("failure.empty_reply") : c.message);

/** Reasoning effort values offered, in order; null is "the service's default" (not sent). */
export const EFFORTS: (string | null)[] = [null, "none", "minimal", "low", "medium", "high"];

/** A local service's row: running ones first, then by catalogue order. */
export const localRows = (found: LocalService[], services: ServicePreset[], role: Role): LocalService[] => {
  const ids = new Set(localFor(services, role).map((s) => s.id));
  return found.filter((f) => ids.has(f.id)).sort((a, b) => Number(b.running) - Number(a.running));
};
