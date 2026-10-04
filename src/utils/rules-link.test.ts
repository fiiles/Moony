import { describe, expect, it } from 'vitest';
import { RULES_PAGE_PATH, newRuleHref, parseNewRuleSearch } from './rules-link';

describe('newRuleHref', () => {
  it('points at the rules page with the new-rule flag', () => {
    expect(newRuleHref({})).toBe(`${RULES_PAGE_PATH}?new=1`);
  });

  it('carries the payee and category, encoded', () => {
    const href = newRuleHref({ payee: ' Dr. Max & Co ', categoryId: 'cat_health' });
    expect(href).toBe(`${RULES_PAGE_PATH}?new=1&payee=Dr.+Max+%26+Co&category=cat_health`);
  });

  it('skips blank values', () => {
    expect(newRuleHref({ payee: '   ', categoryId: null })).toBe(`${RULES_PAGE_PATH}?new=1`);
  });
});

describe('parseNewRuleSearch', () => {
  it('round-trips what newRuleHref builds', () => {
    const href = newRuleHref({ payee: 'AUSTRIAN AIR 123 & more', categoryId: 'cat_travel' });
    const search = href.slice(href.indexOf('?'));
    expect(parseNewRuleSearch(search)).toEqual({
      payee: 'AUSTRIAN AIR 123 & more',
      categoryId: 'cat_travel',
    });
  });

  it('ignores a query string that does not ask for a new rule', () => {
    expect(parseNewRuleSearch('')).toBeNull();
    expect(parseNewRuleSearch('?payee=LIDL')).toBeNull();
    expect(parseNewRuleSearch('new=0&payee=LIDL')).toBeNull();
  });

  it('accepts a bare new flag and bounds an oversized payee', () => {
    expect(parseNewRuleSearch('new=1')).toEqual({ payee: '', categoryId: null });
    const long = 'x'.repeat(500);
    expect(parseNewRuleSearch(`?new=1&payee=${long}`)?.payee).toHaveLength(200);
  });
});
