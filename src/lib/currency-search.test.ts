import { describe, expect, it } from 'vitest';
import { currencyMatchScore, normalizeSearch } from './currency-search';

describe('normalizeSearch', () => {
  it('lowercases, trims and strips diacritics', () => {
    expect(normalizeSearch('  Česká Koruna ')).toBe('ceska koruna');
  });
});

describe('currencyMatchScore', () => {
  const czk = ['Česká koruna', 'Czech Koruna'];
  const eur = ['Euro', 'Euro'];
  const usd = ['Americký dolar', 'US Dollar'];

  it('matches everything for an empty query', () => {
    expect(currencyMatchScore('CZK', czk, '')).toBeGreaterThan(0);
    expect(currencyMatchScore('CZK', czk, '   ')).toBeGreaterThan(0);
  });

  it('finds a currency by its code, case-insensitively', () => {
    expect(currencyMatchScore('EUR', eur, 'eur')).toBeGreaterThan(0);
    expect(currencyMatchScore('EUR', eur, 'EUR')).toBe(1);
  });

  it('finds a currency by its localized name, ignoring diacritics', () => {
    expect(currencyMatchScore('CZK', czk, 'ceska')).toBeGreaterThan(0);
    expect(currencyMatchScore('CZK', czk, 'koruna')).toBeGreaterThan(0);
  });

  it('finds a currency by its English name when the UI is localized', () => {
    expect(currencyMatchScore('USD', usd, 'dollar')).toBeGreaterThan(0);
  });

  it('rejects non-matching queries', () => {
    expect(currencyMatchScore('EUR', eur, 'xyz')).toBe(0);
    expect(currencyMatchScore('CZK', czk, 'usd')).toBe(0);
  });

  it('ranks an exact code above a code prefix above a name match', () => {
    const exact = currencyMatchScore('EUR', eur, 'eur');
    const prefix = currencyMatchScore('EUR', eur, 'eu');
    // "dollar" is a name word of USD, not a code
    const name = currencyMatchScore('USD', usd, 'dol');
    expect(exact).toBeGreaterThan(prefix);
    expect(prefix).toBeGreaterThan(name);
  });

  it('ranks a name-word start above a mid-word match', () => {
    const wordStart = currencyMatchScore('CZK', czk, 'kor');
    const midWord = currencyMatchScore('CZK', czk, 'oru');
    expect(wordStart).toBeGreaterThan(midWord);
    expect(midWord).toBeGreaterThan(0);
  });
});
