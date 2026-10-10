/** Direction of a change as the UI shows it (▲ gain, ▼ loss, no glyph when flat). */
export type ChangeDirection = 'up' | 'down' | 'flat';

/**
 * Direction of a ratio change (0.0042 = 0,42 %) at the precision it is displayed with, so a
 * value that prints as "0,0 %" is never green or red.
 */
export function changeDirection(ratio: number | null | undefined, digits: number): ChangeDirection {
  if (ratio == null || !Number.isFinite(ratio)) return 'flat';
  const scaled = Math.round(Math.abs(ratio) * 100 * 10 ** digits);
  if (scaled === 0) return 'flat';
  return ratio > 0 ? 'up' : 'down';
}

/** Placeholder put into a translated sentence where a change badge goes. */
export const CHANGE_MARK = '';

/** Splits a translated sentence around `CHANGE_MARK`; null when the placeholder is missing. */
export function splitOnMark(text: string): [string, string] | null {
  const at = text.indexOf(CHANGE_MARK);
  if (at < 0) return null;
  return [text.slice(0, at).trim(), text.slice(at + CHANGE_MARK.length).trim()];
}
