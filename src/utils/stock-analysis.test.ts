import { describe, expect, it } from 'vitest';
import { FILTER_ALL, chosenTagIds, selectPositions, tagTotals } from './stock-analysis';

const growth = { id: 'growth' };
const value = { id: 'value' };
const usa = { id: 'usa' };
const europe = { id: 'europe' };

const strategy = { key: 'strategy', tags: [growth, value] };
const region = { key: 'region', tags: [usa, europe] };

function position(
  id: string,
  tags: { id: string }[],
  currentValue: number,
  gainLoss = 0,
  dividendYield = 0
) {
  return { id, tags, currentValue, gainLoss, dividendYield };
}

describe('chosenTagIds', () => {
  it('collects the tag chosen in each group that filters', () => {
    expect(chosenTagIds([strategy, region], { strategy: 'growth', region: 'europe' })).toEqual([
      'growth',
      'europe',
    ]);
  });

  it('skips groups set to all or never touched', () => {
    expect(chosenTagIds([strategy, region], { strategy: FILTER_ALL })).toEqual([]);
    expect(chosenTagIds([strategy, region], {})).toEqual([]);
    expect(chosenTagIds([strategy, region], { region: 'usa' })).toEqual(['usa']);
  });

  it('ignores a choice that is no longer one of the group tags', () => {
    // The tag was deleted, or the key belongs to a group that is gone
    expect(chosenTagIds([strategy], { strategy: 'deleted', gone: 'growth' })).toEqual([]);
  });
});

describe('selectPositions', () => {
  const apple = position('apple', [growth, usa], 100);
  const nestle = position('nestle', [value, europe], 50);
  const sap = position('sap', [growth, europe], 30);
  const loose = position('loose', [], 10);
  const all = [apple, nestle, sap, loose];

  it('selects every position when no tag is chosen', () => {
    expect(selectPositions(all, [])).toEqual(all);
  });

  it('keeps the positions that carry the chosen tag', () => {
    expect(selectPositions(all, ['growth']).map((p) => p.id)).toEqual(['apple', 'sap']);
  });

  it('requires every chosen tag: one per group, AND across groups', () => {
    expect(selectPositions(all, ['growth', 'europe']).map((p) => p.id)).toEqual(['sap']);
    expect(selectPositions(all, ['value', 'usa'])).toEqual([]);
  });

  it('never selects untagged positions once a tag is chosen', () => {
    expect(selectPositions(all, ['usa']).some((p) => p.id === 'loose')).toBe(false);
  });

  it('does not mutate or alias the input', () => {
    const copy = selectPositions(all, []);
    expect(copy).not.toBe(all);
    expect(all).toHaveLength(4);
  });
});

describe('tagTotals', () => {
  const positions = [
    position('apple', [growth, usa], 1200, 200, 24),
    position('nestle', [value, europe], 800, -100, 40),
    position('sap', [growth, europe], 500, 50, 0),
    position('loose', [], 300, 30, 6),
  ];

  it('sums the positions carrying the tag', () => {
    expect(tagTotals(positions, 'growth')).toEqual({
      count: 2,
      value: 1700,
      gainLoss: 250,
      // cost = value − gain = 1450; 250 / 1450
      gainLossPercent: (250 / 1450) * 100,
      dividend: 24,
    });
  });

  it('counts a position once for each tag it carries', () => {
    expect(tagTotals(positions, 'europe').count).toBe(2);
    expect(tagTotals(positions, 'usa').count).toBe(1);
  });

  it('reports a loss as a negative percentage of the cost', () => {
    const totals = tagTotals(positions, 'value');
    // cost = 800 − (−100) = 900
    expect(totals.gainLoss).toBe(-100);
    expect(totals.gainLossPercent).toBeCloseTo((-100 / 900) * 100, 10);
  });

  it('is all zeros for a tag no position carries', () => {
    expect(tagTotals(positions, 'unused')).toEqual({
      count: 0,
      value: 0,
      gainLoss: 0,
      gainLossPercent: 0,
      dividend: 0,
    });
  });

  it('gives no percentage without a cost basis', () => {
    // Value equals the gain: nothing was paid in, so there is nothing to divide by
    expect(tagTotals([position('gift', [growth], 100, 100)], 'growth').gainLossPercent).toBe(0);
  });

  it('follows a narrowed selection', () => {
    const selected = selectPositions(positions, ['europe']);
    expect(tagTotals(selected, 'growth').count).toBe(1);
    expect(tagTotals(selected, 'growth').value).toBe(500);
    expect(tagTotals(selected, 'usa').count).toBe(0);
  });
});
