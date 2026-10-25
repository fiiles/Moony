import { describe, expect, it } from 'vitest';
import type { StockSplit } from '@shared/schema';
import {
  defaultSplitChoice,
  isoDayToUnix,
  preSplitHint,
  ratioText,
  splitActionOrder,
} from './stock-splits';

const DAY = 86_400;
const JUNE_10_2024 = Date.UTC(2024, 5, 10) / 1000;

function split(overrides: Partial<StockSplit>): StockSplit {
  return {
    id: 'split-1',
    ticker: 'NVDA',
    effectiveDay: JUNE_10_2024,
    numerator: '10',
    denominator: '1',
    source: 'yahoo',
    status: 'applied',
    detectedAt: JUNE_10_2024,
    resolvedAt: JUNE_10_2024,
    appliedCount: 3,
    preSplitCount: 3,
    verdict: 'unknown',
    ...overrides,
  };
}

describe('ratioText', () => {
  it('writes new shares before old ones', () => {
    expect(ratioText('10', '1')).toBe('10:1');
    expect(ratioText('1', '3')).toBe('1:3');
  });
});

describe('preSplitHint', () => {
  it('returns the applied split dated after the entered day', () => {
    const s = split({});
    expect(preSplitHint([s], JUNE_10_2024 - DAY)).toBe(s);
  });

  it('returns null on or after the split day', () => {
    const s = split({});
    expect(preSplitHint([s], JUNE_10_2024)).toBeNull();
    expect(preSplitHint([s], JUNE_10_2024 + DAY)).toBeNull();
  });

  it('ignores pending and dismissed splits', () => {
    expect(
      preSplitHint(
        [split({ status: 'pending' }), split({ id: 'split-2', status: 'dismissed' })],
        JUNE_10_2024 - DAY
      )
    ).toBeNull();
  });

  it('picks the latest applied split when several follow the day', () => {
    const early = split({ id: 'early', effectiveDay: JUNE_10_2024 - 400 * DAY });
    const late = split({ id: 'late', effectiveDay: JUNE_10_2024 });
    expect(preSplitHint([late, early], JUNE_10_2024 - 500 * DAY)).toBe(late);
    expect(preSplitHint([early, late], JUNE_10_2024 - 500 * DAY)).toBe(late);
  });

  it('returns null without splits', () => {
    expect(preSplitHint([], JUNE_10_2024)).toBeNull();
  });
});

describe('defaultSplitChoice', () => {
  it('maps the verdict to the emphasised action', () => {
    expect(defaultSplitChoice('unadjusted')).toBe('apply');
    expect(defaultSplitChoice('adjusted')).toBe('dismiss');
    expect(defaultSplitChoice('unknown')).toBeNull();
  });
});

describe('splitActionOrder', () => {
  it('puts the emphasised action first', () => {
    expect(splitActionOrder('unadjusted')).toEqual(['apply', 'dismiss']);
    expect(splitActionOrder('adjusted')).toEqual(['dismiss', 'apply']);
  });

  it('keeps apply, dismiss when Moony cannot tell', () => {
    expect(splitActionOrder('unknown')).toEqual(['apply', 'dismiss']);
  });
});

describe('isoDayToUnix', () => {
  it('reads a yyyy-mm-dd value as its UTC midnight', () => {
    expect(isoDayToUnix('2024-06-10')).toBe(JUNE_10_2024);
  });

  it('rejects anything else', () => {
    expect(isoDayToUnix('')).toBeNull();
    expect(isoDayToUnix(undefined)).toBeNull();
    expect(isoDayToUnix('2024-6-10')).toBeNull();
    expect(isoDayToUnix('2024-02-30')).toBeNull();
  });
});
