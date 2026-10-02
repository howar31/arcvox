// The input level as the widget shows it while it listens.

/** How often the level is sampled, in ms. */
export const TICK = 50;
/** The level range shown, in decibels: nothing at the floor and under, full from the ceiling up. */
const FLOOR = -55, CEIL = -15;

/** 0 to 1 for an RMS value, even in decibels: speech moves a linear scale very little. */
export function mapped(rms: number): number {
  if (rms <= 0) return 0;
  return Math.min(1, Math.max(0, (20 * Math.log10(rms) - FLOOR) / (CEIL - FLOOR)));
}

/** The next shown level: it follows a rise quickly and a fall slowly. */
export function eased(prev: number, next: number): number {
  return prev + (next - prev) * (next > prev ? 0.7 : 0.3);
}

/** The last levels, newest first. */
export class LevelHistory {
  private values: number[];

  constructor(size: number) {
    this.values = new Array(size).fill(0);
  }

  push(level: number) {
    this.values.unshift(level);
    this.values.pop();
  }

  /** The level `age` samples ago; between two samples the change is eased. */
  at(age: number): number {
    if (age <= 0) return this.values[0];
    const i = Math.floor(age), f = age - i;
    const a = this.values[i] ?? 0, b = this.values[i + 1] ?? 0;
    return a + (b - a) * (0.5 - 0.5 * Math.cos(Math.PI * f));
  }

  clear() {
    this.values.fill(0);
  }
}
