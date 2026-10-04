/** Series order of the 100 % flow bar (design system §11): s1…s4, then Ostatní. */
export const FLOW_SERIES = ['var(--s1)', 'var(--s2)', 'var(--s3)', 'var(--s4)', 'var(--s-other)'];

/** Hatched remainder segment ("Zbývá k investování"). */
export const FLOW_REST =
  'repeating-linear-gradient(135deg, var(--paper) 0 4px, var(--well-2) 4px 6px)';

/** Legend swatch for the i-th colored segment, or the hatch for the remainder. */
export function flowColor(index: number, kind?: 'rest'): string {
  return kind === 'rest'
    ? 'repeating-linear-gradient(135deg, var(--paper) 0 2px, var(--well-3) 2px 3px)'
    : FLOW_SERIES[Math.min(index, FLOW_SERIES.length - 1)];
}
