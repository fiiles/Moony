import { describe, expect, it } from 'vitest';
import {
  horizonAxisLabels,
  payoffIndex,
  roundMarks,
  roundMilestones,
} from './projection-milestones';

describe('payoffIndex', () => {
  it('finds the first zero after a positive start', () => {
    const pts = [
      { value: 1, liabilities: 300 },
      { value: 2, liabilities: 150 },
      { value: 3, liabilities: 0 },
      { value: 4, liabilities: 0 },
    ];
    expect(payoffIndex(pts)).toBe(2);
  });

  it('is null without liabilities or when they never reach zero', () => {
    expect(payoffIndex([{ value: 1, liabilities: 0 }])).toBeNull();
    expect(
      payoffIndex([
        { value: 1, liabilities: 100 },
        { value: 2, liabilities: 50 },
      ])
    ).toBeNull();
    expect(payoffIndex([])).toBeNull();
  });
});

describe('roundMarks', () => {
  it('lists the 1·2·5 marks between the two values', () => {
    expect(roundMarks(2_846_900, 12_400_000)).toEqual([5_000_000, 10_000_000]);
    expect(roundMarks(900_000, 2_100_000)).toEqual([1_000_000, 2_000_000]);
  });

  it('is empty when nothing grows', () => {
    expect(roundMarks(5, 5)).toEqual([]);
    expect(roundMarks(10, 2)).toEqual([]);
  });
});

describe('roundMilestones', () => {
  const values = [2_846_900, 3_400_000, 4_100_000, 5_200_000, 6_900_000, 9_000_000, 12_400_000];

  it('maps each mark to the first point at or above it, skipping the ends', () => {
    expect(roundMilestones(values)).toEqual(
      [
        { value: 5_000_000, index: 3 },
        { value: 10_000_000, index: 6 },
      ].filter((m) => m.index < values.length - 1)
    );
  });

  it('skips indexes already taken by another milestone', () => {
    expect(roundMilestones(values, [3])).toEqual([]);
  });

  it('keeps first, middle and last when there are too many', () => {
    const many = [100_000, 1_000_000, 2_000_000, 5_000_000, 10_000_000, 20_000_000, 60_000_000];
    const picked = roundMilestones(many);
    expect(picked.map((m) => m.value)).toEqual([1_000_000, 5_000_000, 20_000_000]);
  });
});

describe('horizonAxisLabels', () => {
  it('starts with today and spaces the years evenly', () => {
    expect(horizonAxisLabels(2026, 20, 'Dnes')).toEqual(['Dnes', '2031', '2036', '2041', '2046']);
    expect(horizonAxisLabels(2026, 5, 'Dnes')).toEqual(['Dnes', '2027', '2029', '2030', '2031']);
  });
});
