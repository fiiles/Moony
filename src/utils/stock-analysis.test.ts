import { describe, expect, it } from 'vitest';
import type { StockTag } from '@shared/schema';
import {
  FILTER_ALL,
  changedPositions,
  chosenTagIds,
  heldPositions,
  initialActiveId,
  isHeld,
  moveConflicts,
  pickGroupTag,
  selectPositions,
  tagTotals,
  toggleFreeTag,
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

describe('isHeld / heldPositions', () => {
  it('holds a position with a positive quantity, whole or fractional', () => {
    expect(isHeld({ quantity: '10' })).toBe(true);
    expect(isHeld({ quantity: '0.00000001' })).toBe(true);
  });

  it('does not hold a sold-out, negative or unreadable quantity', () => {
    expect(isHeld({ quantity: '0' })).toBe(false);
    expect(isHeld({ quantity: '0.00000000' })).toBe(false);
    expect(isHeld({ quantity: '-1' })).toBe(false);
    expect(isHeld({ quantity: '' })).toBe(false);
    expect(isHeld({ quantity: 'n/a' })).toBe(false);
  });

  it('keeps the held positions in order and does not touch the input', () => {
    const all = [
      { id: 'a', quantity: '5' },
      { id: 'closed', quantity: '0' },
      { id: 'b', quantity: '1.5' },
    ];
    const held = heldPositions(all);
    expect(held.map((p) => p.id)).toEqual(['a', 'b']);
    expect(all).toHaveLength(3);
    expect(held).not.toBe(all);
  });

  it('is empty when every position is sold out', () => {
    expect(heldPositions([{ quantity: '0' }, { quantity: '0.0' }])).toEqual([]);
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

describe('pickGroupTag', () => {
  const growthTag = tag('growth', 'strategy');
  const valueTag = tag('value', 'strategy');
  const usaTag = tag('usa', 'region');
  const watchTag = tag('watch', null);
  const all = [growthTag, valueTag, usaTag, watchTag];

  it('replaces the tag the position has in the group and leaves the other tags alone', () => {
    expect(pickGroupTag(['growth', 'usa', 'watch'], all, 'strategy', 'value')).toEqual([
      'usa',
      'watch',
      'value',
    ]);
  });

  it('gives a position without a tag in the group the chosen one', () => {
    expect(pickGroupTag(['usa'], all, 'strategy', 'growth')).toEqual(['usa', 'growth']);
    expect(pickGroupTag([], all, 'strategy', 'growth')).toEqual(['growth']);
  });

  it('removes the tag of the group for null, and only that one', () => {
    expect(pickGroupTag(['growth', 'usa', 'watch'], all, 'strategy', null)).toEqual([
      'usa',
      'watch',
    ]);
    expect(pickGroupTag(['usa'], all, 'strategy', null)).toEqual(['usa']);
  });

  it('keeps the choice when the chosen tag is picked again', () => {
    expect(pickGroupTag(['growth', 'usa'], all, 'strategy', 'growth')).toEqual(['usa', 'growth']);
  });

  it('ignores a tag that is not a tag of the group (deleted or moved meanwhile)', () => {
    expect(pickGroupTag(['growth'], all, 'strategy', 'ghost')).toEqual(['growth']);
    expect(pickGroupTag(['growth'], all, 'strategy', 'usa')).toEqual(['growth']);
    expect(pickGroupTag(['growth'], all, 'strategy', 'watch')).toEqual(['growth']);
  });

  it('does not change the list it is given', () => {
    const current = ['growth', 'usa'];
    pickGroupTag(current, all, 'strategy', 'value');
    expect(current).toEqual(['growth', 'usa']);
  });
});

describe('toggleFreeTag', () => {
  const growthTag = tag('growth', 'strategy');
  const watchTag = tag('watch', null);
  const incomeTag = tag('income', null);
  const all = [growthTag, watchTag, incomeTag];

  it('adds a tag without a group that the position does not have', () => {
    expect(toggleFreeTag(['growth'], all, 'watch')).toEqual(['growth', 'watch']);
    expect(toggleFreeTag(['watch'], all, 'income')).toEqual(['watch', 'income']);
  });

  it('removes one it already has, and only that one', () => {
    expect(toggleFreeTag(['growth', 'watch', 'income'], all, 'watch')).toEqual([
      'growth',
      'income',
    ]);
  });

  it('ignores a tag that belongs to a group or does not exist', () => {
    expect(toggleFreeTag(['watch'], all, 'growth')).toEqual(['watch']);
    expect(toggleFreeTag(['watch'], all, 'ghost')).toEqual(['watch']);
  });

  it('does not change the list it is given', () => {
    const current = ['watch'];
    toggleFreeTag(current, all, 'income');
    expect(current).toEqual(['watch']);
  });
});

describe('changedPositions', () => {
  const originals = [
    { id: 'a', tags: [{ id: 'growth' }, { id: 'usa' }] },
    { id: 'b', tags: [] },
    { id: 'c', tags: [{ id: 'value' }] },
  ];

  it('is empty without drafts and for drafts equal to the saved tags', () => {
    expect(changedPositions(originals, new Map())).toEqual([]);
    const same = new Map([
      ['a', ['growth', 'usa']],
      ['b', []],
    ]);
    expect(changedPositions(originals, same)).toEqual([]);
  });

  it('ignores the order of the tags', () => {
    expect(changedPositions(originals, new Map([['a', ['usa', 'growth']]]))).toEqual([]);
  });

  it('lists the positions whose tags differ, in the order of the positions', () => {
    const drafts = new Map([
      ['c', []],
      ['a', ['growth']],
      ['b', ['watch']],
    ]);
    expect(changedPositions(originals, drafts)).toEqual(['a', 'b', 'c']);
  });

  it('treats a tag added and removed again as unchanged', () => {
    expect(changedPositions(originals, new Map([['b', []]]))).toEqual([]);
  });

  it('ignores a draft of a position that is not in the list', () => {
    expect(changedPositions(originals, new Map([['ghost', ['growth']]]))).toEqual([]);
  });
});

describe('initialActiveId', () => {
  const positions = [
    { id: 'a', tags: [{ id: 'growth' }] },
    { id: 'b', tags: [] },
    { id: 'c', tags: [] },
  ];

  it('opens on the first id it is given', () => {
    expect(initialActiveId(positions, ['c', 'b'])).toBe('c');
    expect(initialActiveId(positions, ['a'])).toBe('a');
  });

  it('skips ids that are not in the list', () => {
    expect(initialActiveId(positions, ['ghost', 'c'])).toBe('c');
  });

  it('opens on the first position without a tag when it is given no id', () => {
    expect(initialActiveId(positions, [])).toBe('b');
    expect(initialActiveId(positions, ['ghost'])).toBe('b');
  });

  it('opens on the first position when all of them have a tag', () => {
    const tagged = [
      { id: 'x', tags: [{ id: 'growth' }] },
      { id: 'y', tags: [{ id: 'value' }] },
    ];
    expect(initialActiveId(tagged, [])).toBe('x');
  });

  it('is null for no positions', () => {
    expect(initialActiveId([], [])).toBeNull();
    expect(initialActiveId([], ['a'])).toBeNull();
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
