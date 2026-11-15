import { describe, expect, it } from 'vitest';
import { createFormatters } from '@/lib/format';
import {
  approxMoneyOptions,
  approxOf,
  exactMoneyOptions,
  movesWithRates,
  WHOLE,
} from './money-rule';

describe('exactMoneyOptions', () => {
  it('keeps the minor units of an amount that has them', () => {
    expect(exactMoneyOptions(2.06, 'EUR')).toEqual({ decimals: 2 });
    expect(exactMoneyOptions(129.9, 'CZK')).toEqual({ decimals: 2 });
    expect(exactMoneyOptions(-0.5, 'USD')).toEqual({ decimals: 2 });
    expect(exactMoneyOptions(1.234, 'KWD')).toEqual({ decimals: 3 });
  });

  it('drops them on a whole amount', () => {
    expect(exactMoneyOptions(80, 'EUR')).toEqual({ decimals: 0 });
    expect(exactMoneyOptions(500, 'CZK')).toEqual({ decimals: 0 });
    expect(exactMoneyOptions(-1249, 'CZK')).toEqual({ decimals: 0 });
    expect(exactMoneyOptions(0, 'USD')).toEqual({ decimals: 0 });
  });

  it('reads an amount that is whole at the minor units as whole (float noise, a third decimal)', () => {
    expect(exactMoneyOptions(0.1 + 0.2 + 79.7, 'EUR')).toEqual({ decimals: 0 });
    expect(exactMoneyOptions(12.001, 'USD')).toEqual({ decimals: 0 });
    expect(exactMoneyOptions(11.999, 'USD')).toEqual({ decimals: 0 });
  });

  it('has no decimals for a currency without minor units', () => {
    expect(exactMoneyOptions(1262.4, 'JPY')).toEqual({ decimals: 0 });
  });

  it('treats a missing or unknown currency as two minor units', () => {
    expect(exactMoneyOptions(1.5, '')).toEqual({ decimals: 2 });
    expect(exactMoneyOptions(1.5, 'XXX')).toEqual({ decimals: 2 });
  });

  it('formats as the rule reads: "129,90 Kč", "500 Kč", "2,06 EUR", "80 EUR"', () => {
    const fmt = createFormatters('cs', 'CZK');
    const text = (amount: number, currency: string) =>
      fmt.money(amount, currency, exactMoneyOptions(amount, currency)).replace(/\s/g, ' ');
    expect(text(129.9, 'CZK')).toBe('129,90 Kč');
    expect(text(500, 'CZK')).toBe('500 Kč');
    expect(text(2.06, 'EUR')).toBe('2,06 EUR');
    expect(text(80, 'EUR')).toBe('80 EUR');
  });
});

describe('approxOf', () => {
  const base = { amount: 1000, currency: 'CZK', mainCurrency: 'USD', converted: 42.6 };

  it('is the converted amount of a record in another currency', () => {
    expect(approxOf(base)).toEqual({ kind: 'approx', value: 42.6 });
  });

  it('is nothing for a record in the main currency', () => {
    expect(approxOf({ ...base, currency: 'USD', converted: 1000 })).toEqual({ kind: 'none' });
  });

  it('is nothing for a zero amount, with or without a rate', () => {
    expect(approxOf({ ...base, amount: 0, converted: 0 })).toEqual({ kind: 'none' });
    expect(approxOf({ ...base, amount: 0, converted: null })).toEqual({ kind: 'none' });
  });

  it('says that the currency has no rate', () => {
    expect(approxOf({ ...base, converted: null })).toEqual({ kind: 'noRate' });
  });

  it('is nothing while the rates load', () => {
    expect(approxOf({ ...base, converted: null, pending: true })).toEqual({ kind: 'none' });
  });

  it('is nothing when the converted amount rounds to nothing', () => {
    // 0.4 yen is no yen; 0.004 dollars is no cent
    expect(approxOf({ ...base, mainCurrency: 'JPY', converted: 0.4 })).toEqual({ kind: 'none' });
    expect(approxOf({ ...base, converted: 0.004 })).toEqual({ kind: 'none' });
    expect(approxOf({ ...base, converted: 0.21 })).toEqual({ kind: 'approx', value: 0.21 });
  });

  it('keeps the sign of a negative amount', () => {
    expect(approxOf({ ...base, amount: -1000, converted: -42.6 })).toEqual({
      kind: 'approx',
      value: -42.6,
    });
  });
});

describe('approxMoneyOptions', () => {
  it('is whole from one unit up', () => {
    expect(approxMoneyOptions(42.6, 'USD')).toEqual(WHOLE);
    expect(approxMoneyOptions(-1, 'EUR')).toEqual(WHOLE);
    // 0.996 rounds to one whole unit
    expect(approxMoneyOptions(0.996, 'USD')).toEqual(WHOLE);
  });

  it('keeps the currency decimals under one unit', () => {
    expect(approxMoneyOptions(0.21, 'USD')).toEqual({ decimals: 2 });
    expect(approxMoneyOptions(-0.5, 'EUR')).toEqual({ decimals: 2 });
  });

  it('stays whole for a currency without decimals', () => {
    expect(approxMoneyOptions(0.6, 'JPY')).toEqual(WHOLE);
    expect(approxMoneyOptions(0.6, 'CZK')).toEqual(WHOLE);
  });
});

describe('movesWithRates', () => {
  it('is true when anything is held in another currency than the main one (A8)', () => {
    expect(movesWithRates(['CZK', 'EUR'], 'CZK')).toBe(true);
    expect(movesWithRates(['czk', ' CZK '], 'CZK')).toBe(false);
    expect(movesWithRates([], 'USD')).toBe(false);
    expect(movesWithRates([null, undefined, ''], 'USD')).toBe(false);
  });
});
