import { describe, expect, it } from 'vitest';
import type { StockTag } from '@shared/schema';
import type { AnalysisPosition } from '@/utils/stock-analysis';
import { holdingRows, portfolioConcentration, tagRows, type TagGroupRows } from './tables-model';

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
    periodReturnOf: (id: string) => ({ growth: 8, value: -2, core: 5 })[id] ?? null,
    filterActive: false,
  };

  it('lists every tag by group, then the tags without a group', () => {
    const rows = tagRows(input);
    expect(rows.map((r) => r.tag.id)).toEqual(['growth', 'value', 'unused', 'core']);
    expect(rows.map((r) => r.groupName)).toEqual(['Strategy', 'Strategy', 'Strategy', null]);
  });

  it('sums the value at the end, and shares it out of the whole portfolio', () => {
    const [growthRow, valueRow] = tagRows(input);
    expect(growthRow).toMatchObject({ count: 2, value: 500, share: 0.5, periodReturn: 8 });
    // The only position has no known value at the end
    expect(valueRow).toMatchObject({ count: 1, value: null, share: null, periodReturn: -2 });
  });

  it('keeps the unrealized gain and the dividend as of today', () => {
    const [growthRow, valueRow] = tagRows(input);
    // 40 gained on a cost of 360
    expect(growthRow.gainPercent).toBeCloseTo((40 / 360) * 100);
    expect(growthRow.dividend).toBe(4);
    expect(valueRow.gainPercent).toBeCloseTo((-20 / 120) * 100);
    expect(valueRow.dividend).toBe(0);
  });

  it('lists a tag without positions with nothing to show', () => {
    const unusedRow = tagRows(input)[2];
    expect(unusedRow).toMatchObject({ count: 0, value: null, share: null, periodReturn: null });
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
