import { describe, expect, it } from 'vitest';
import { mergeConversionQuality } from './conversion-quality';

describe('mergeConversionQuality', () => {
  it('lists every missing currency once and adds up the misses', () => {
    expect(
      mergeConversionQuality(
        { missingCurrencies: ['KWD', 'JPY'], missingCount: 2, estimated: false },
        undefined,
        { missingCurrencies: ['KWD'], missingCount: 3, estimated: true }
      )
    ).toEqual({ missingCurrencies: ['JPY', 'KWD'], missingCount: 5, estimated: true });
  });

  it('is undefined while no part is known', () => {
    expect(mergeConversionQuality(undefined, undefined)).toBeUndefined();
  });
});
