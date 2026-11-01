// What a dropdown lists, and when it offers a search field.

export interface Choice { value: string; label: string; help?: string; disabled?: boolean }
export interface ChoiceGroup { label?: string; choices: Choice[] }

// A list whose length depends on the machine or the user is filtered early; a fixed list only when it is long.
const SEARCH_FROM = { variable: 3, fixed: 8 };

export const wantsSearch = (count: number, variable: boolean): boolean => count >= SEARCH_FROM[variable ? "variable" : "fixed"];

export function filterGroups(groups: ChoiceGroup[], query: string): ChoiceGroup[] {
  const q = query.trim().toLowerCase();
  if (!q) return groups;
  return groups
    .map((g) => ({ ...g, choices: g.choices.filter((c) => (c.label + " " + (c.help ?? "")).toLowerCase().includes(q)) }))
    .filter((g) => g.choices.length > 0);
}
