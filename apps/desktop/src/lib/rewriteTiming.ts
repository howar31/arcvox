// The timing of the change from the recognised text to the final text in the panel.

/** How long a unit of text takes to close or open, in ms. */
export const MOVE = 600;
/** How long the removed text stays struck through before it closes. */
export const LEAD = MOVE * 0.8;
/** How long the added text stays highlighted after it opened. */
export const REST = 300;
/** The whole change. */
export const TOTAL = LEAD + MOVE + REST + MOVE;
/** How long the final text is left to read before a message takes the panel. */
const READ = 600;

/**
 * How long a message waits so that the change is seen first; 0 when nothing is changing.
 * `refinedAt` is when the final text arrived, on the same clock as `now`.
 */
export function messageDelay(transcript: string, refined: string | null, refinedAt: number, now: number, still: boolean): number {
  if (still || refined === null || !transcript || refined === transcript) return 0;
  return Math.max(0, Math.round(refinedAt + TOTAL + READ - now));
}
