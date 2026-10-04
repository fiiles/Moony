import { describe, expect, it } from 'vitest';
import type {
  CalculatedDefaults,
  ProjectionSettings,
  ProjectionTimelinePoint,
} from '@shared/schema';
import {
  ASSET_CLASSES,
  DEFAULT_RATES,
  contributionToStore,
  defaultRate,
  joinList,
  parseRate,
  rateDigits,
  rateToStore,
  resolveAssumptions,
  summarizeAssumptions,
} from './projection-assumptions';

function row(
  assetType: ProjectionSettings['assetType'],
  yearlyGrowthRate: string,
  monthlyContribution = '0',
  enabled = true
): ProjectionSettings {
  return {
    id: assetType,
    assetType,
    yearlyGrowthRate,
    monthlyContribution,
    contributionCurrency: 'CZK',
    enabled,
  };
}

const calculated: CalculatedDefaults = { savingsRate: 3.5, bondsRate: 4.25 };

function point(values: Partial<ProjectionTimelinePoint>): ProjectionTimelinePoint {
  return {
    date: 0,
    totalAssets: 0,
    totalLiabilities: 0,
    netWorth: 0,
    savings: 0,
    investments: 0,
    crypto: 0,
    bonds: 0,
    realEstate: 0,
    otherAssets: 0,
    loans: 0,
    ...values,
  };
}

describe('parseRate', () => {
  it('reads an explicit rate, including an explicit zero', () => {
    expect(parseRate('7')).toBe(7);
    expect(parseRate('0')).toBe(0);
    expect(parseRate('0.00')).toBe(0);
    expect(parseRate(' 3.5 ')).toBe(3.5);
    expect(parseRate('-2')).toBe(-2);
  });

  it('treats empty or unusable text as "not set"', () => {
    expect(parseRate('')).toBeUndefined();
    expect(parseRate('   ')).toBeUndefined();
    expect(parseRate('abc')).toBeUndefined();
    expect(parseRate('Infinity')).toBeUndefined();
    expect(parseRate(undefined)).toBeUndefined();
  });
});

describe('resolveAssumptions', () => {
  it('falls back to the built-in defaults and the calculated savings/bonds rates', () => {
    const a = resolveAssumptions(undefined, calculated);
    expect(a.savings).toEqual({ rate: 3.5, explicit: false, contribution: 0 });
    expect(a.bonds).toEqual({ rate: 4.25, explicit: false, contribution: 0 });
    expect(a.investments.rate).toBe(DEFAULT_RATES.investments);
    expect(a.crypto.rate).toBe(7);
    expect(a.real_estate.rate).toBe(3);
    expect(a.other_assets.rate).toBe(0);
  });

  it('honours an explicit 0 % for savings and bonds', () => {
    const a = resolveAssumptions([row('savings', '0'), row('bonds', '0.00')], calculated);
    expect(a.savings).toMatchObject({ rate: 0, explicit: true });
    expect(a.bonds).toMatchObject({ rate: 0, explicit: true });
  });

  it('treats an empty stored rate as "use the calculated default"', () => {
    const a = resolveAssumptions([row('savings', ''), row('investments', '')], calculated);
    expect(a.savings).toMatchObject({ rate: 3.5, explicit: false });
    expect(a.investments).toMatchObject({ rate: 7, explicit: false });
  });

  it('reads stored rates and contributions', () => {
    const a = resolveAssumptions(
      [row('investments', '9', '2000'), row('real_estate', '0', '0'), row('savings', '2', '500')],
      calculated
    );
    expect(a.investments).toEqual({ rate: 9, explicit: true, contribution: 2000 });
    expect(a.real_estate).toMatchObject({ rate: 0, explicit: true });
    expect(a.savings).toEqual({ rate: 2, explicit: true, contribution: 500 });
  });

  it('ignores disabled rows like the backend does', () => {
    const a = resolveAssumptions([row('investments', '12', '1000', false)], calculated);
    expect(a.investments).toEqual({ rate: 7, explicit: false, contribution: 0 });
  });

  it('survives missing calculated defaults and unusable contributions', () => {
    const a = resolveAssumptions([row('crypto', '5', 'oops')], undefined);
    expect(a.savings.rate).toBe(0);
    expect(a.bonds.rate).toBe(0);
    expect(a.crypto.contribution).toBe(0);
  });
});

describe('summarizeAssumptions', () => {
  it('lists only the classes the user holds or contributes to', () => {
    const a = resolveAssumptions(undefined, calculated);
    const s = summarizeAssumptions(a, point({ investments: 1000, realEstate: 5000 }));
    expect(s.rateGroups).toEqual([
      { rate: 7, keys: ['investments'] },
      { rate: 3, keys: ['real_estate'] },
    ]);
    expect(s.monthlyContribution).toBe(0);
  });

  it('includes a class that is empty today but has a contribution', () => {
    const a = resolveAssumptions([row('crypto', '7', '1000')], calculated);
    const s = summarizeAssumptions(a, point({}));
    expect(s.rateGroups).toEqual([{ rate: 7, keys: ['crypto'] }]);
    expect(s.monthlyContribution).toBe(1000);
  });

  it('groups classes that share a rate, in class order', () => {
    const a = resolveAssumptions(undefined, calculated);
    const s = summarizeAssumptions(
      a,
      point({ investments: 1, crypto: 1, realEstate: 1, savings: 1 })
    );
    expect(s.rateGroups).toEqual([
      { rate: 3.5, keys: ['savings'] },
      { rate: 7, keys: ['investments', 'crypto'] },
      { rate: 3, keys: ['real_estate'] },
    ]);
  });

  it('sums contributions over the classes that take them (not real estate)', () => {
    const a = resolveAssumptions(
      [
        row('savings', '2', '30000'),
        row('investments', '7', '50000'),
        row('crypto', '7', '10000'),
        row('real_estate', '3', '99999'),
      ],
      calculated
    );
    const s = summarizeAssumptions(a, point({ savings: 1, investments: 1, crypto: 1 }));
    expect(s.monthlyContribution).toBe(90000);
  });

  it('is empty without a current point', () => {
    const a = resolveAssumptions(undefined, calculated);
    expect(summarizeAssumptions(a, undefined)).toEqual({ rateGroups: [], monthlyContribution: 0 });
  });

  it('covers every asset class that has a value field', () => {
    expect(ASSET_CLASSES.map((c) => c.key)).toEqual([
      'savings',
      'investments',
      'crypto',
      'bonds',
      'real_estate',
      'other_assets',
    ]);
  });
});

describe('rateToStore', () => {
  it('keeps an explicit number, zero included', () => {
    expect(rateToStore('7.5')).toBe('7.5');
    expect(rateToStore('0')).toBe('0');
    expect(rateToStore('0.00')).toBe('0');
    expect(rateToStore(' 12 ')).toBe('12');
  });

  it('stores an empty field as "not set"', () => {
    expect(rateToStore('')).toBe('');
    expect(rateToStore('  ')).toBe('');
    expect(rateToStore('abc')).toBe('');
  });
});

describe('contributionToStore', () => {
  const toCzk = (v: number) => v * 25; // 1 EUR = 25 CZK

  it('keeps the stored CZK value when the field was not touched (no rounding drift)', () => {
    expect(contributionToStore('40', '40', 1000, toCzk)).toBe('1000');
  });

  it('converts an edited display-currency amount back to CZK', () => {
    expect(contributionToStore('100', '40', 1000, toCzk)).toBe('2500');
  });

  it('rounds the converted amount to cents', () => {
    expect(contributionToStore('1.234', '0', 0, (v) => v * 24.567)).toBe('30.32');
  });

  it('stores an empty or unusable field as 0', () => {
    expect(contributionToStore('', '40', 1000, toCzk)).toBe('0');
    expect(contributionToStore('abc', '40', 1000, toCzk)).toBe('0');
  });
});

describe('joinList', () => {
  it('joins with the locale conjunction', () => {
    expect(joinList(['a', 'b', 'c'], 'en-US')).toBe('a, b, and c');
    // Czech glues the conjunction to the next word with a no-break space
    expect(joinList(['a', 'b'], 'cs-CZ').replace(/\u00a0/g, ' ')).toBe('a a b');
  });

  it('handles one and no item', () => {
    expect(joinList(['a'], 'en-US')).toBe('a');
    expect(joinList([], 'en-US')).toBe('');
  });
});

describe('defaultRate', () => {
  it('uses the calculated rate for savings and bonds and a constant for the rest', () => {
    expect(defaultRate('savings', calculated)).toBe(3.5);
    expect(defaultRate('bonds', calculated)).toBe(4.25);
    expect(defaultRate('investments', calculated)).toBe(7);
    expect(defaultRate('real_estate', calculated)).toBe(3);
    expect(defaultRate('savings', undefined)).toBe(0);
  });
});

describe('rateDigits', () => {
  it('shows only the digits a rate needs', () => {
    expect(rateDigits(7)).toBe(0);
    expect(rateDigits(0)).toBe(0);
    expect(rateDigits(3.5)).toBe(1);
    expect(rateDigits(4.25)).toBe(2);
    expect(rateDigits(2.123)).toBe(2);
  });
});
