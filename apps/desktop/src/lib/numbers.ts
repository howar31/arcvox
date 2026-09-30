export interface NumericField { key: string; value: unknown; min?: number; max?: number }

/** Keys whose value is not a finite number, or falls outside the given bounds. */
export function invalidFields(fields: NumericField[]): string[] {
  return fields
    .filter(({ value, min, max }) => {
      if (typeof value !== "number" || !Number.isFinite(value)) return true;
      return (min !== undefined && value < min) || (max !== undefined && value > max);
    })
    .map((f) => f.key);
}
