import type { Formatters } from '@/lib/format';

/** How a signed value is coloured: gain, loss, or neither. */
export type Tone = 'gain' | 'loss' | 'flat';

/**
 * Tone of a value at the precision it is shown with, so a value that prints as "0,0" is neither
 * green nor red.
 */
export function toneOf(value: number | null | undefined, digits = 1): Tone {
  if (value == null || !Number.isFinite(value)) return 'flat';
  if (Math.round(Math.abs(value) * 10 ** digits) === 0) return 'flat';
  return value > 0 ? 'gain' : 'loss';
}

/** The glyph that carries the direction of a signed value next to it (design system §2). */
export function arrowOf(tone: Tone): '▲ ' | '▼ ' | '' {
  return tone === 'gain' ? '▲ ' : tone === 'loss' ? '▼ ' : '';
}

const NBSP = ' ';
const MINUS = '−';

/**
 * Percentage points the way the design system writes signed changes: "+ 2,5 p. b.", "− 0,8 p. b."
 * (true minus, no-break spaces), "0,0 p. b." when the value rounds to zero. `signed: false` gives
 * the magnitude only, for a spot that already shows an arrow. `unit` is the locale's "p. b.".
 */
export function formatPoints(
  number: Formatters['number'],
  value: number,
  unit: string,
  opts: { signed?: boolean; digits?: number } = {}
): string {
  const digits = opts.digits ?? 1;
  const text = number(Math.abs(value), {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
  const tone = toneOf(value, digits);
  const sign = opts.signed === false || tone === 'flat' ? '' : tone === 'gain' ? '+' : MINUS;
  return `${sign}${sign ? NBSP : ''}${text}${NBSP}${unit}`;
}
