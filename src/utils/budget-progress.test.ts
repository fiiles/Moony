import { describe, it, expect } from 'vitest';
import { budgetMultiplier, budgetUsage } from './budget-progress';

describe('budgetMultiplier', () => {
  it('scales the monthly limit to the viewed period', () => {
    expect(budgetMultiplier('monthly')).toBe(1);
    expect(budgetMultiplier('quarterly')).toBe(3);
    expect(budgetMultiplier('yearly')).toBe(12);
  });
});

describe('budgetUsage', () => {
  it('fills the bar by the share of the limit that is spent', () => {
    expect(budgetUsage(2500, 10000, 'monthly')).toEqual({
      limitCzk: 10000,
      percent: 25,
      fill: 25,
      over: false,
    });
  });

  it('compares a quarterly or yearly spend with the scaled monthly limit', () => {
    expect(budgetUsage(15000, 10000, 'quarterly')).toMatchObject({
      limitCzk: 30000,
      percent: 50,
      over: false,
    });
    expect(budgetUsage(60000, 10000, 'yearly')).toMatchObject({ limitCzk: 120000, percent: 50 });
  });

  it('caps the fill at 100 % but reports the real percentage once over the limit', () => {
    expect(budgetUsage(15000, 10000, 'monthly')).toEqual({
      limitCzk: 10000,
      percent: 150,
      fill: 100,
      over: true,
    });
  });

  it('is exactly at the limit, not over it, when spent equals the limit', () => {
    expect(budgetUsage(10000, 10000, 'monthly')).toMatchObject({
      percent: 100,
      fill: 100,
      over: false,
    });
  });

  it('has no usage to show without a positive limit', () => {
    expect(budgetUsage(500, 0, 'monthly')).toEqual({
      limitCzk: 0,
      percent: 0,
      fill: 0,
      over: false,
    });
  });

  it('rounds the percentage to a whole number', () => {
    expect(budgetUsage(1, 3, 'monthly').percent).toBe(33);
  });
});
