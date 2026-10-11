import { describe, expect, it } from 'vitest';
import type { StockTag } from '@shared/schema';
import {
  FILTER_ALL,
  chosenTagIds,
  moveConflicts,
  nextTagIds,
  selectPositions,
  tagTotals,
} from './stock-analysis';

const growth = { id: 'growth' };
const value = { id: 'value' };
const usa = { id: 'usa' };
const europe = { id: 'europe' };

const strategy = { key: 'strategy', tags: [growth, value] };
const region = { key: 'region', tags: [usa, europe] };

function position(
  id: string,
  tags: { id: string }[],
  currentValue: number | null,
  gainLoss: number | null = 0,
  yearlyDividendAmount: number | null = 0,
  price: string | null = '1'
) {
  return { id, tags, currentValue, gainLoss, yearlyDividendAmount, price };
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

  it('has no value when its priced positions hold value but none converted', () => {
    const kwd = position('kwd', [growth], null, null, null);
    expect(tagTotals([kwd], 'growth').value).toBeNull();
    // an unpriced position holds no value: a real 0
    expect(tagTotals([position('new', [growth], null, null, null, null)], 'growth').value).toBe(0);
    // one converted position makes it known again
    expect(tagTotals([kwd, position('sap', [growth], 500)], 'growth').value).toBe(500);
  });

  it('gives no percentage without a cost basis', () => {
    // Value equals the gain: nothing was paid in, so there is nothing to divide by
    expect(tagTotals([position('gift', [growth], 100, 100)], 'growth').gainLossPercent).toBe(0);
  });

  it('sums every known amount: an unknown value leaves out its dividend, an unknown cost its gain', () => {
    const withGap = [...positions, position('kwd', [growth], null, null, 99)];
    const totals = tagTotals(withGap, 'growth');
    expect(totals.count).toBe(3);
    expect(totals.value).toBe(1700);
    expect(totals.dividend).toBe(24);
    const unknownCost = [...positions, position('gift', [growth], 100, null, 5)];
    expect(tagTotals(unknownCost, 'growth')).toEqual({
      count: 3,
      value: 1800,
      gainLoss: 250,
      // the percent stays on the positions with a known gain: 250 / 1450
      gainLossPercent: (250 / 1450) * 100,
      dividend: 29,
    });
  });

  it('follows a narrowed selection', () => {
    const selected = selectPositions(positions, ['europe']);
    expect(tagTotals(selected, 'growth').count).toBe(1);
    expect(tagTotals(selected, 'growth').value).toBe(500);
    expect(tagTotals(selected, 'usa').count).toBe(0);
  });
});

function tag(id: string, groupId: string | null): StockTag {
  return { id, name: id, color: null, groupId, createdAt: 0 };
}

describe('nextTagIds', () => {
  const growthTag = tag('growth', 'strategy');
  const valueTag = tag('value', 'strategy');
  const usaTag = tag('usa', 'region');
  const europeTag = tag('europe', 'region');
  const watchTag = tag('watch', null);
  const incomeTag = tag('income', null);
  const all = [growthTag, valueTag, usaTag, europeTag, watchTag, incomeTag];

  it('keeps the tag of a group left on keep', () => {
    const next = nextTagIds(
      [growthTag, usaTag],
      { strategy: 'keep', region: 'keep' },
      [],
      true,
      all
    );
    expect(next).toEqual(['growth', 'usa']);
  });

  it('removes the tag of a group set to none, and only that one', () => {
    const next = nextTagIds(
      [growthTag, usaTag],
      { strategy: 'none', region: 'keep' },
      [],
      true,
      all
    );
    expect(next).toEqual(['usa']);
  });

  it('replaces the tag of a group with the chosen one', () => {
    const next = nextTagIds(
      [growthTag, usaTag],
      { strategy: 'value', region: 'keep' },
      [],
      true,
      all
    );
    expect(next).toEqual(['usa', 'value']);
  });

  it('gives a position without a tag the chosen one and leaves other groups alone', () => {
    expect(nextTagIds([], { strategy: 'growth', region: 'none' }, [], true, all)).toEqual([
      'growth',
    ]);
    expect(nextTagIds([], { strategy: 'keep', region: 'keep' }, [], true, all)).toEqual([]);
  });

  it('sets the free tags exactly for a single position', () => {
    const next = nextTagIds([growthTag, watchTag], { strategy: 'keep' }, ['income'], true, all);
    // watch is unchecked, so it goes; income is added
    expect(next).toEqual(['growth', 'income']);
    expect(nextTagIds([watchTag], { strategy: 'keep' }, [], true, all)).toEqual([]);
  });

  it('adds the free tags to several positions and keeps the ones they have', () => {
    const next = nextTagIds(
      [growthTag, watchTag],
      { strategy: 'keep' },
      ['income', 'watch'],
      false,
      all
    );
    expect(next).toEqual(['growth', 'watch', 'income']);
    // with nothing checked the existing free tags stay
    expect(nextTagIds([watchTag], { strategy: 'keep' }, [], false, all)).toEqual(['watch']);
  });

  it('applies keep, none and replace per group across several positions', () => {
    const picks = { strategy: 'value', region: 'keep' };
    expect(nextTagIds([growthTag, europeTag], picks, [], false, all)).toEqual(['europe', 'value']);
    expect(nextTagIds([], picks, [], false, all)).toEqual(['value']);
  });

  it('keeps a tag of a group the picks do not mention', () => {
    expect(nextTagIds([europeTag], { strategy: 'growth' }, [], true, all)).toEqual([
      'europe',
      'growth',
    ]);
  });

  it('ignores a pick that is not a tag of its group (deleted or moved meanwhile)', () => {
    // 'ghost' no longer exists and 'usa' belongs to another group: the current tag stays
    expect(nextTagIds([growthTag], { strategy: 'ghost' }, [], true, all)).toEqual(['growth']);
    expect(nextTagIds([growthTag], { strategy: 'usa' }, [], true, all)).toEqual(['growth']);
  });

  it('ignores a free id that is unknown or belongs to a group, and lists no tag twice', () => {
    expect(nextTagIds([], {}, ['ghost', 'growth', 'watch', 'watch'], true, all)).toEqual(['watch']);
  });
});

describe('moveConflicts', () => {
  const growthTag = tag('growth', 'strategy');
  const valueTag = tag('value', 'strategy');
  const usaTag = tag('usa', 'region');
  const positions = [
    { companyName: 'Apple', tags: [growthTag, usaTag] },
    { companyName: 'SAP', tags: [growthTag] },
    { companyName: 'Nestlé', tags: [valueTag, usaTag] },
    { companyName: 'Visa', tags: [usaTag] },
  ];

  it('lists the positions that would carry two tags of the group', () => {
    // growth into region: Apple has usa already
    expect(moveConflicts(positions, 'growth', 'region')).toEqual(['Apple']);
    // usa into strategy: Apple (growth) and Nestlé (value) would carry two there
    expect(moveConflicts(positions, 'usa', 'strategy')).toEqual(['Apple', 'Nestlé']);
  });

  it('is empty when the positions that carry the tag have nothing else in the group', () => {
    // SAP has only growth and Visa only usa: moving either tag to the other group is fine
    expect(moveConflicts([positions[1]], 'growth', 'region')).toEqual([]);
    expect(moveConflicts([positions[3]], 'usa', 'strategy')).toEqual([]);
  });

  it('ignores positions that do not carry the moved tag', () => {
    // SAP has no usa and Visa no strategy tag, so only Apple and Nestlé can conflict
    expect(moveConflicts(positions, 'usa', 'strategy')).toEqual(['Apple', 'Nestlé']);
    expect(moveConflicts(positions, 'value', 'region')).toEqual(['Nestlé']);
  });

  it('is empty for a tag nobody carries and for a group nobody uses', () => {
    expect(moveConflicts(positions, 'unused', 'region')).toEqual([]);
    expect(moveConflicts(positions, 'growth', 'empty-group')).toEqual([]);
    expect(moveConflicts([], 'growth', 'region')).toEqual([]);
  });
});
