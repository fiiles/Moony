import { describe, expect, it } from 'vitest';
import {
  allocationInput,
  classRowState,
  propertyChange,
  topTickers,
  visibleClasses,
  INVESTMENT_CLASSES,
} from './wealth-rows';

const DAY = 86_400;
const T = Date.UTC(2026, 9, 16) / 1000;

describe('visibleClasses', () => {
  it('keeps the spec order and drops hidden classes; the optional flags default to shown', () => {
    expect(INVESTMENT_CLASSES).toEqual(['investments', 'crypto', 'bonds', 'otherAssets']);
    expect(visibleClasses({ investments: true, bonds: true })).toEqual([
      'investments',
      'crypto',
      'bonds',
      'otherAssets',
    ]);
    expect(
      visibleClasses({ investments: true, bonds: false, crypto: false, otherAssets: true })
    ).toEqual(['investments', 'otherAssets']);
  });
});

describe('topTickers', () => {
  const rows = [
    { ticker: 'CEZ', quantity: 10, value: 168_400 },
    { ticker: 'VWCE', quantity: 426, value: 1_182_600 },
    { ticker: 'OLD', quantity: 0, value: 0 },
    { ticker: 'CHF1', quantity: 3, value: null },
    { ticker: 'CSPX', quantity: 20, value: 612_300 },
    { ticker: 'AAPL', quantity: 5, value: 286_900 },
  ];
  it('names the open positions by value, unknown values last, the rest counted', () => {
    expect(topTickers(rows, 3)).toBe('VWCE, CSPX, AAPL +2');
    expect(topTickers(rows.slice(0, 2), 3)).toBe('VWCE, CEZ');
    expect(topTickers([], 3)).toBe('');
  });
});

describe('propertyChange', () => {
  it('is the last revaluation against the one before it, dated by the last', () => {
    expect(
      propertyChange([
        { valuedAt: T - 400 * DAY, value: '2823000' },
        { valuedAt: T - 11 * DAY, value: '2860000' },
      ])
    ).toEqual({ ratio: 37_000 / 2_823_000, day: T - 11 * DAY });
  });
  it('has no ratio with one valuation or a zero base, and is null with none; order does not matter', () => {
    expect(propertyChange([{ valuedAt: T, value: '100' }])).toEqual({ ratio: null, day: T });
    expect(
      propertyChange([
        { valuedAt: T - DAY, value: '0' },
        { valuedAt: T, value: '5' },
      ])
    ).toEqual({ ratio: null, day: T });
    expect(
      propertyChange([
        { valuedAt: T, value: '5' },
        { valuedAt: T - DAY, value: '4' },
      ])
    ).toEqual({ ratio: 0.25, day: T });
    expect(propertyChange([])).toBeNull();
  });
});

describe('allocationInput', () => {
  const labels = {
    realEstate: 'Nemovitosti',
    investments: 'Akcie',
    bonds: 'Dluhopisy',
    crypto: 'Krypto',
    otherAssets: 'Ostatní aktiva',
    savings: 'Na účtech',
  };
  it('lists the six classes in the fixed order, leaving out unknown and empty ones', () => {
    const display = {
      realEstate: 2_860_000,
      investments: 2_650_000,
      bonds: 488_000,
      crypto: 214_500,
      otherAssets: 0,
      savings: 214_380,
    };
    const unknown = {
      realEstate: false,
      investments: false,
      bonds: false,
      crypto: true,
      otherAssets: false,
      savings: false,
    };
    expect(allocationInput(display, unknown, labels).map((s) => s.key)).toEqual([
      'realEstate',
      'investments',
      'bonds',
      'savings',
    ]);
    expect(allocationInput(display, unknown, labels)[0]).toEqual({
      key: 'realEstate',
      label: 'Nemovitosti',
      value: 2_860_000,
    });
  });
});

describe('classRowState', () => {
  it('is empty only without any record and without a value', () => {
    expect(classRowState({ records: 0, value: 0, unknown: false })).toBe('empty');
  });
  it('keeps a class with only closed or matured records reachable: records, not open ones, decide', () => {
    expect(classRowState({ records: 3, value: 0, unknown: false })).toBe('valued');
    expect(classRowState({ records: 0, value: 5, unknown: false })).toBe('valued');
  });
  it('is unknown when held but nothing converted, however many records there are', () => {
    expect(classRowState({ records: 2, value: 0, unknown: true })).toBe('unknown');
    expect(classRowState({ records: 0, value: 0, unknown: true })).toBe('unknown');
  });
});
