import { describe, expect, it } from 'vitest';
import type { PlanCategoryRow } from '@/utils/spending-plans';
import { categoryShares, planBars, sparseLabels } from './plan-detail';

describe('planBars', () => {
  const axis = ['2026-01', '2026-02', '2026-03'];

  it('mutes nothing without a selected month', () => {
    expect(planBars(axis, [10, 0, 5], null)).toEqual([
      { month: '2026-01', value: 10, muted: false },
      { month: '2026-02', value: 0, muted: false },
      { month: '2026-03', value: 5, muted: false },
    ]);
  });

  it('mutes every month but the selected one', () => {
    expect(planBars(axis, [10, 0, 5], '2026-02').map((bar) => bar.muted)).toEqual([
      true,
      false,
      true,
    ]);
  });

  it('keeps a net refund month negative and reads a missing value as 0', () => {
    expect(planBars(axis, [-4, 3], null).map((bar) => bar.value)).toEqual([-4, 3, 0]);
  });
});

describe('sparseLabels', () => {
  it('keeps every third label from the first', () => {
    expect(sparseLabels(['a', 'b', 'c', 'd', 'e', 'f', 'g'], 3)).toEqual([
      'a',
      '',
      '',
      'd',
      '',
      '',
      'g',
    ]);
  });
});

describe('categoryShares', () => {
  const row = (key: string, spent: number): PlanCategoryRow => ({
    key,
    categoryId: key || null,
    spent,
    count: 1,
  });

  it('gives each row its share of the total', () => {
    expect(categoryShares([row('a', 75), row('b', 25)]).map((r) => r.share)).toEqual([0.75, 0.25]);
  });

  it('never gives a negative share (a category of refunds)', () => {
    const shares = categoryShares([row('a', 120), row('', -20)]).map((r) => r.share);
    expect(shares).toEqual([1.2, 0]);
  });

  it('gives 0 when the total is not positive', () => {
    expect(categoryShares([row('a', 0)]).map((r) => r.share)).toEqual([0]);
    expect(categoryShares([row('a', -5)]).map((r) => r.share)).toEqual([0]);
  });

  it('keeps the rows and their order', () => {
    const rows = [row('a', 10), row('b', 5)];
    expect(categoryShares(rows).map((r) => r.key)).toEqual(['a', 'b']);
  });
});
