export interface NumericField { key: string; value: unknown; min?: number; max?: number; integer?: boolean }

/** Keys whose value is not a finite number, is fractional where `integer` is
 * set, or falls outside the given bounds. */
export function invalidFields(fields: NumericField[]): string[] {
  return fields
    .filter(({ value, min, max, integer }) => {
      if (typeof value !== "number" || !Number.isFinite(value)) return true;
      if (integer && !Number.isInteger(value)) return true;
      return (min !== undefined && value < min) || (max !== undefined && value > max);
    })
    .map((f) => f.key);
}
