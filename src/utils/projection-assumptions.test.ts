import { describe, expect, it } from 'vitest';
import type { CalculatedDefaults } from '@shared/schema';
import {
  contributionToStore,
  defaultRate,
  joinList,
  parseRate,
  rateDigits,
  rateToStore,
  toProjectionPoint,
} from './projection-assumptions';

const calculated: CalculatedDefaults = { savingsRate: 3.5, bondsRate: 4.25 };

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
  const stored = { amount: 500, currency: 'EUR' };

  it('keeps the stored amount and currency when the field was not touched', () => {
    expect(contributionToStore('40', '40', stored, 'CZK')).toEqual({
      monthlyContribution: '500',
      contributionCurrency: 'EUR',
    });
  });

  it('stores an edited amount as typed, in the currency the field showed', () => {
    expect(contributionToStore('100', '40', stored, 'CZK')).toEqual({
      monthlyContribution: '100',
      contributionCurrency: 'CZK',
    });
  });

  it('rounds the typed amount to cents', () => {
    expect(contributionToStore('1.234', '0', { amount: 0, currency: '' }, 'EUR')).toEqual({
      monthlyContribution: '1.23',
      contributionCurrency: 'EUR',
    });
  });

  it('stores an empty or unusable field as 0 in the shown currency', () => {
    const zero = { monthlyContribution: '0', contributionCurrency: 'CZK' };
    expect(contributionToStore('', '40', stored, 'CZK')).toEqual(zero);
    expect(contributionToStore('abc', '40', stored, 'CZK')).toEqual(zero);
  });

  it('uses the shown currency for an untouched row that has none stored', () => {
    expect(contributionToStore('0', '0', { amount: 0, currency: '' }, 'EUR')).toEqual({
      monthlyContribution: '0',
      contributionCurrency: 'EUR',
    });
  });
});

describe('toProjectionPoint', () => {
  it('reads the decimal text of every amount as a number', () => {
    const p = toProjectionPoint({
      date: 5,
      totalAssets: '1250.50',
      totalLiabilities: '250.00',
      netWorth: '1000.50',
      savings: '1000.50',
      investments: '0.00',
      crypto: '0.00',
      bonds: '0.00',
      realEstate: '250.00',
      otherAssets: '0.00',
      loans: '250.00',
    });
    expect(p.date).toBe(5);
    expect(p.netWorth).toBe(1000.5);
    expect(p.realEstate).toBe(250);
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
