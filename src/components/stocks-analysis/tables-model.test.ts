import { describe, expect, it } from 'vitest';
import type { StockTag } from '@shared/schema';
import type { AnalysisPosition } from '@/utils/stock-analysis';
import {
  holdingRows,
  portfolioConcentration,
  tagGroupTotals,
  tagRows,
  type TagGroupRows,
} from './tables-model';

function tag(id: string, name: string, groupId: string | null = 'g'): StockTag {
  return { id, name, color: null, groupId, createdAt: 0 } as unknown as StockTag;
}

function position(
  id: string,
  tags: StockTag[],
  extra: Partial<AnalysisPosition> = {}
): AnalysisPosition {
  return {
    id,
    ticker: id.toUpperCase(),
    companyName: `Company ${id}`,
    price: '1',
    tags,
    currentValue: 100,
    gainLoss: 10,
    gainLossPercent: 10,
    yearlyDividendAmount: 2,
    ...extra,
  } as unknown as AnalysisPosition;
}

const growth = tag('growth', 'Growth');
const value = tag('value', 'Value');
const core = tag('core', 'Core', null);
const unused = tag('unused', 'Unused');

const groups: TagGroupRows[] = [
  { key: 'g', label: 'Strategy', tags: [growth, value, unused] },
  { key: 'free', label: 'Tags', tags: [core], free: true },
];

// Series figures over the period: return in percent and the money gained
const PERIOD: Record<string, { periodReturn: number; periodGain: number | null }> = {
  'tag:growth': { periodReturn: 8, periodGain: 40 },
  'tag:value': { periodReturn: -2, periodGain: -2 },
  'tag:core': { periodReturn: 5, periodGain: 5 },
  'tag-group:g': { periodReturn: 6.5, periodGain: 38 },
};
const periodOf = (key: string) => PERIOD[key] ?? null;

describe('tagRows', () => {
  const positions = [
    position('a', [growth, core], { currentValue: 100, gainLoss: 10, yearlyDividendAmount: 1 }),
    position('b', [growth], { currentValue: 300, gainLoss: 30, yearlyDividendAmount: 3 }),
    position('c', [value], { currentValue: 100, gainLoss: -20, yearlyDividendAmount: 0 }),
  ];
  const ends: Record<string, number | null> = { a: 150, b: 350, c: null };
  const input = {
    groups,
    selected: positions,
    endValueOf: (id: string) => ends[id] ?? null,
    endTotal: 1000,
    periodOf,
    filterActive: false,
  };

  it('lists every tag by group, then the tags without a group', () => {
    const rows = tagRows(input);
    expect(rows.map((r) => r.tag.id)).toEqual(['growth', 'value', 'unused', 'core']);
    expect(rows.map((r) => r.groupName)).toEqual(['Strategy', 'Strategy', 'Strategy', null]);
    expect(rows.map((r) => r.groupKey)).toEqual(['g', 'g', 'g', 'free']);
  });

  it('sums the value at the end, and shares it out of the whole portfolio', () => {
    const [growthRow, valueRow] = tagRows(input);
    expect(growthRow).toMatchObject({ count: 2, value: 500, share: 0.5, periodReturn: 8 });
    // The only position has no known value at the end
    expect(valueRow).toMatchObject({ count: 1, value: null, share: null, periodReturn: -2 });
  });

  it('carries the return over the period as a percent and an amount', () => {
    const [growthRow, valueRow] = tagRows(input);
    expect(growthRow).toMatchObject({ periodReturn: 8, periodGain: 40 });
    expect(valueRow).toMatchObject({ periodReturn: -2, periodGain: -2 });
  });

  it('keeps the unrealized gain and the dividend as of today', () => {
    const [growthRow, valueRow] = tagRows(input);
    // 40 gained on a cost of 360
    expect(growthRow.gain).toBeCloseTo(40);
    expect(growthRow.gainPercent).toBeCloseTo((40 / 360) * 100);
    expect(growthRow.dividend).toBe(4);
    expect(valueRow.gain).toBeCloseTo(-20);
    expect(valueRow.gainPercent).toBeCloseTo((-20 / 120) * 100);
    expect(valueRow.dividend).toBe(0);
  });

  it('lists a tag without positions with nothing to show', () => {
    const unusedRow = tagRows(input)[2];
    expect(unusedRow).toMatchObject({
      count: 0,
      value: null,
      share: null,
      periodReturn: null,
      periodGain: null,
    });
  });

  it('counts only the selected positions and hides tags without one under a filter', () => {
    const rows = tagRows({ ...input, selected: [positions[0]], filterActive: true });
    expect(rows.map((r) => r.tag.id)).toEqual(['growth', 'core']);
    expect(rows[0]).toMatchObject({ count: 1, value: 150, share: 0.15 });
  });

  it('has no share without a portfolio value', () => {
    expect(tagRows({ ...input, endTotal: null })[0].share).toBeNull();
    expect(tagRows({ ...input, endTotal: 0 })[0].share).toBeNull();
  });
});

describe('tagGroupTotals', () => {
  const positions = [
    position('a', [growth, core], { currentValue: 100, gainLoss: 10, yearlyDividendAmount: 1 }),
    position('b', [growth], { currentValue: 300, gainLoss: 30, yearlyDividendAmount: 3 }),
    position('c', [value], { currentValue: 100, gainLoss: -20, yearlyDividendAmount: 0 }),
    position('d', [core], { currentValue: 500, gainLoss: 50, yearlyDividendAmount: 5 }),
  ];
  const ends: Record<string, number | null> = { a: 150, b: 350, c: null, d: 500 };
  const input = {
    groups,
    selected: positions,
    endValueOf: (id: string) => ends[id] ?? null,
    endTotal: 1000,
    periodOf,
    filterActive: false,
  };

  it('sums the positions carrying a tag of the group, once each', () => {
    const [total] = tagGroupTotals(input);
    expect(total).toMatchObject({
      groupKey: 'g',
      label: 'Strategy',
      count: 3,
      value: 500,
      share: 0.5,
      dividend: 4,
    });
    // 20 gained on a cost of 480 (c's value is unknown at the end but known today)
    expect(total.gain).toBeCloseTo(20);
    expect(total.gainPercent).toBeCloseTo((20 / 480) * 100);
  });

  it("takes the period return of the group's own series", () => {
    const [total] = tagGroupTotals(input);
    expect(total).toMatchObject({ periodReturn: 6.5, periodGain: 38 });
    expect(tagGroupTotals({ ...input, periodOf: () => null })[0]).toMatchObject({
      periodReturn: null,
      periodGain: null,
    });
  });

  it('has none for the tags without a group, which overlap', () => {
    expect(tagGroupTotals(input).map((t) => t.groupKey)).toEqual(['g']);
  });

  it('has none for a group with fewer than two tags holding a selected position', () => {
    expect(tagGroupTotals({ ...input, selected: [positions[0], positions[1]] })).toEqual([]);
  });

  it('counts only the selected positions', () => {
    const [total] = tagGroupTotals({ ...input, selected: [positions[0], positions[2]] });
    expect(total).toMatchObject({ count: 2, value: 150, share: 0.15, dividend: 1 });
  });
});

describe('holdingRows', () => {
  const positions = [
    position('a', [], { companyName: 'Škoda Auto', ticker: 'SKA' }),
    position('b', [], { companyName: 'Apple', ticker: 'AAPL' }),
    position('c', [], { companyName: 'Cemex', ticker: 'CX' }),
    position('d', [], { companyName: 'Dolby', ticker: 'DLB' }),
  ];
  const ends: Record<string, number | null> = { a: 100, b: 300, c: null, d: 100 };
  const endValueOf = (id: string) => ends[id] ?? null;

  it('puts the largest value first, equal values in their order and unknown values last', () => {
    expect(holdingRows(positions, endValueOf, '').map((p) => p.id)).toEqual(['b', 'a', 'd', 'c']);
  });

  it('searches company and ticker, ignoring case and diacritics', () => {
    expect(holdingRows(positions, endValueOf, 'skoda').map((p) => p.id)).toEqual(['a']);
    expect(holdingRows(positions, endValueOf, 'aapl').map((p) => p.id)).toEqual(['b']);
    expect(holdingRows(positions, endValueOf, 'nothing')).toEqual([]);
  });

  it('does not reorder the input', () => {
    const copy = [...positions];
    holdingRows(positions, endValueOf, '');
    expect(positions).toEqual(copy);
  });
});

describe('portfolioConcentration', () => {
  it('measures the concentration of the known positive values', () => {
    const ends: Record<string, number | null> = { a: 50, b: 30, c: 20, d: null, e: 0 };
    const result = portfolioConcentration(
      ['a', 'b', 'c', 'd', 'e'].map((id) => ({ id })),
      (id) => ends[id]
    );
    expect(result?.top5Share).toBe(1);
    expect(result?.effective).toBeCloseTo(1 / (0.25 + 0.09 + 0.04));
  });

  it('has nothing to measure without a value', () => {
    expect(portfolioConcentration([{ id: 'a' }], () => null)).toBeNull();
    expect(portfolioConcentration([], () => 1)).toBeNull();
  });
});
