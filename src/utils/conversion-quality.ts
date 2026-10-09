import type { ConversionQuality } from '@shared/schema';

/**
 * One quality for a view built from several responses: every missing currency once (sorted),
 * the missing conversions added up, estimated when any part is. Absent parts are skipped;
 * `undefined` while no part is known.
 */
export function mergeConversionQuality(
  ...parts: (ConversionQuality | undefined)[]
): ConversionQuality | undefined {
  const known = parts.filter((q): q is ConversionQuality => q !== undefined);
  if (known.length === 0) return undefined;
  return {
    missingCurrencies: [...new Set(known.flatMap((q) => q.missingCurrencies))].sort(),
    missingCount: known.reduce((sum, q) => sum + q.missingCount, 0),
    estimated: known.some((q) => q.estimated),
  };
}
