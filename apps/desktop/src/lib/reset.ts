/** A way back to a setting's default, offered while the setting differs from it. */
export interface Reset {
  /** Says what the default is. */
  hint: string;
  run: () => void;
}

function at(obj: unknown, keys: string[]): unknown {
  return keys.reduce<unknown>((o, k) => (o as Record<string, unknown> | null | undefined)?.[k], obj);
}

/**
 * The reset of the value at `path` (member names joined by dots) of `target`;
 * undefined while it equals the one in `defaults`, or while either is not loaded.
 */
export function resetOf(target: object | null, defaults: object | null, path: string, describe: (value: unknown) => string): Reset | undefined {
  if (!target || !defaults) return undefined;
  const keys = path.split(".");
  const standard = at(defaults, keys);
  if (JSON.stringify(at(target, keys) ?? null) === JSON.stringify(standard ?? null)) return undefined;
  return {
    hint: describe(standard),
    run: () => {
      const holder = at(target, keys.slice(0, -1)) as Record<string, unknown>;
      // Through JSON: the defaults may be held in a proxy, which cannot be cloned as a structure.
      holder[keys[keys.length - 1]] = standard === undefined ? undefined : JSON.parse(JSON.stringify(standard));
    },
  };
}

/** One action for all the given resets; undefined when there is none. */
export function together(resets: (Reset | undefined)[]): (() => void) | undefined {
  const changed = resets.filter((r) => r !== undefined);
  return changed.length ? () => changed.forEach((r) => r.run()) : undefined;
}
