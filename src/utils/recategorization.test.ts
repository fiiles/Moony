import { describe, expect, it } from 'vitest';
import { changedByPick, formatBreakdown } from './recategorization';

const names: Record<string, string> = {
  dining: 'Stravování',
  groceries: 'Potraviny',
  gifts: 'Dárky',
  fun: 'Zábava',
};
const labels = { uncategorized: 'bez kategorie', more: (n: number) => `+${n} dalších` };
const nameOf = (id: string) => names[id] ?? id;

describe('formatBreakdown', () => {
  it('lists the largest buckets first with the uncategorized label', () => {
    expect(
      formatBreakdown(
        [
          { categoryId: null, count: 2 },
          { categoryId: 'dining', count: 12 },
        ],
        nameOf,
        labels
      )
    ).toBe('12× Stravování · 2× bez kategorie');
  });

  it('folds buckets beyond the limit into "+N dalších"', () => {
    expect(
      formatBreakdown(
        [
          { categoryId: 'dining', count: 20 },
          { categoryId: 'groceries', count: 5 },
          { categoryId: null, count: 2 },
          { categoryId: 'gifts', count: 1 },
          { categoryId: 'fun', count: 1 },
        ],
        nameOf,
        labels
      )
    ).toBe('20× Stravování · 5× Potraviny · 2× bez kategorie · +2 dalších');
  });

  it('is empty for no buckets', () => {
    expect(formatBreakdown([], nameOf, labels)).toBe('');
  });
});

describe('changedByPick', () => {
  const preview = {
    matches: 27,
    byCategory: [
      { categoryId: 'dining', count: 20 },
      { categoryId: 'groceries', count: 5 },
      { categoryId: null, count: 2 },
    ],
  };
  it('subtracts the rows already in the picked category', () => {
    expect(changedByPick(preview, 'groceries')).toBe(22);
    expect(changedByPick(preview, 'dining')).toBe(7);
    expect(changedByPick(preview, 'gifts')).toBe(27);
  });
});
