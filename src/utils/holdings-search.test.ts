import { describe, expect, it } from 'vitest';
import { matchesSearch, normalizeSearchText } from './holdings-search';

describe('normalizeSearchText', () => {
  it('lower-cases, trims and strips diacritics', () => {
    expect(normalizeSearchText('  ŠKODA Auto ')).toBe('skoda auto');
    expect(normalizeSearchText('Česká spořitelna')).toBe('ceska sporitelna');
  });
});

describe('matchesSearch', () => {
  const fields = ['AAPL', 'Apple Inc.', 'Technology', 'Dividend'];

  it('matches everything for an empty or blank term', () => {
    expect(matchesSearch('', fields)).toBe(true);
    expect(matchesSearch('   ', fields)).toBe(true);
  });

  it('matches a substring of any field, case-insensitively', () => {
    expect(matchesSearch('aapl', fields)).toBe(true);
    expect(matchesSearch('APPLE', fields)).toBe(true);
    expect(matchesSearch('tech', fields)).toBe(true);
    expect(matchesSearch('divid', fields)).toBe(true);
  });

  it('does not match text that is in no field', () => {
    expect(matchesSearch('tesla', fields)).toBe(false);
  });

  it('ignores diacritics on both sides', () => {
    expect(matchesSearch('skoda', ['SKODA.PR', 'Škoda Auto'])).toBe(true);
    expect(matchesSearch('škoda', ['SKODA.PR', 'Skoda Auto'])).toBe(true);
  });

  it('requires every word to match somewhere in the row', () => {
    expect(matchesSearch('apple tech', fields)).toBe(true);
    expect(matchesSearch('apple bonds', fields)).toBe(false);
  });

  it('skips missing fields', () => {
    expect(matchesSearch('x', [null, undefined, 'X'])).toBe(true);
    expect(matchesSearch('x', [null, undefined])).toBe(false);
  });
});
