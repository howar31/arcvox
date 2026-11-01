// What a service reports of its use, stage by stage, as the History page shows it.
import type { Stage } from "./api";
import { stageAt, stageName } from "./flowLast";

export const USAGE_COLUMNS = ["total", "input", "audio", "text", "output", "thought", "cached", "tools", "first"] as const;
export type UsageColumn = (typeof USAGE_COLUMNS)[number];
export type UsageRow = { id: "asr" | "llm" | "translate"; name: string } & Record<UsageColumn, number | null>;

const number = (...values: unknown[]): number | null => {
  for (const v of values) if (typeof v === "number") return v;
  return null;
};

/** The tokens of one kind, from a service's list by kind. */
function modality(list: unknown, name: string): number | null {
  if (!Array.isArray(list)) return null;
  const found = (list as { modality?: unknown; tokens?: unknown }[]).find((m) => m.modality === name);
  return found ? number(found.tokens) : null;
}

function numbers(u: Record<string, unknown>): Record<UsageColumn, number | null> {
  return {
    input: number(u.total_input_tokens, u.prompt_tokens, u.input_tokens),
    audio: modality(u.input_tokens_by_modality, "audio"),
    text: modality(u.input_tokens_by_modality, "text"),
    output: number(u.total_output_tokens, u.completion_tokens, u.output_tokens),
    thought: number(u.total_thought_tokens),
    cached: number(u.total_cached_tokens),
    tools: number(u.total_tool_use_tokens),
    first: number(u.initial_input_tokens),
    total: number(u.total_tokens),
  };
}

/** One row per stage that reported its use, in the order they run. */
export function usageRows(stages: Stage[] | null | undefined): UsageRow[] {
  if (!stages) return [];
  const rows: UsageRow[] = [];
  for (const id of ["asr", "llm", "translate"] as const) {
    const usage = stageAt(stages, id)?.detail.usage;
    if (usage && typeof usage === "object") rows.push({ id, name: stageName(id), ...numbers(usage as Record<string, unknown>) });
  }
  return rows;
}

/** The columns some row has a number for. */
export const usageColumns = (rows: UsageRow[]): UsageColumn[] => USAGE_COLUMNS.filter((c) => rows.some((r) => r[c] !== null));

/** All the tokens of a run. */
export const usageSum = (rows: UsageRow[]): number => rows.reduce((sum, r) => sum + (r.total ?? (r.input ?? 0) + (r.output ?? 0)), 0);
