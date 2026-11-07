import { describe, expect, it } from 'vitest';
import { hasUncategorized, showsInterest } from './account-menu';

describe('showsInterest', () => {
  it('shows for an account with zones, whatever its simple rate', () => {
    expect(showsInterest({ hasZoneDesignation: true, interestRate: null })).toBe(true);
  });

  it('shows for a simple rate above zero', () => {
    expect(showsInterest({ hasZoneDesignation: false, interestRate: '1.5' })).toBe(true);
  });

  it('hides for no rate, a zero rate or text that is no number', () => {
    expect(showsInterest({ hasZoneDesignation: false, interestRate: null })).toBe(false);
    expect(showsInterest({ hasZoneDesignation: false, interestRate: '0' })).toBe(false);
    expect(showsInterest({ hasZoneDesignation: false, interestRate: '' })).toBe(false);
    expect(showsInterest({ hasZoneDesignation: false, interestRate: 'abc' })).toBe(false);
  });
});

describe('hasUncategorized', () => {
  it('finds a payment without a category', () => {
    expect(hasUncategorized([{ categoryId: 'food' }, { categoryId: null }])).toBe(true);
  });

  it('has nothing to do when every payment has a category, or there are none', () => {
    expect(hasUncategorized([{ categoryId: 'food' }])).toBe(false);
    expect(hasUncategorized([])).toBe(false);
  });
});
