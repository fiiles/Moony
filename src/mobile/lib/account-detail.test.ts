import { describe, expect, it } from 'vitest';
import { accountDetailSub, hasMonthlyAverage } from './account-detail';

const base = {
  institution: null as string | null,
  currency: 'CZK',
  identifier: null as string | null,
  mainCurrency: 'CZK',
  imported: false,
};

describe('accountDetailSub', () => {
  it('names the bank and the number, without the main currency', () => {
    expect(
      accountDetailSub({
        ...base,
        institution: 'Fio',
        identifier: '2100123456/2010',
        imported: true,
      })
    ).toEqual({ parts: ['Fio', '2100123456/2010'], manual: false });
  });

  it('keeps a currency that is not the main one', () => {
    expect(accountDetailSub({ ...base, institution: 'Wise', currency: 'EUR' })).toEqual({
      parts: ['Wise', 'EUR'],
      manual: false,
    });
  });

  it('calls an account with no bank and no statement kept by hand, the currency only when foreign', () => {
    expect(accountDetailSub(base)).toEqual({ parts: [], manual: true });
    expect(accountDetailSub({ ...base, currency: 'USD' })).toEqual({
      parts: ['USD'],
      manual: true,
    });
  });

  it('an imported account with nothing else to name still says its currency', () => {
    expect(accountDetailSub({ ...base, imported: true })).toEqual({
      parts: ['CZK'],
      manual: false,
    });
  });

  it('ignores blank names and numbers', () => {
    expect(accountDetailSub({ ...base, institution: '  ', identifier: '' })).toEqual({
      parts: [],
      manual: true,
    });
  });
});

describe('hasMonthlyAverage', () => {
  it('needs two months of records when it counts from the first one', () => {
    expect(hasMonthlyAverage(null)).toBe(false);
    expect(hasMonthlyAverage({ months: 1, sinceFirst: true })).toBe(false);
    expect(hasMonthlyAverage({ months: 2, sinceFirst: true })).toBe(true);
  });

  it('a year of records always has one', () => {
    expect(hasMonthlyAverage({ months: 12, sinceFirst: false })).toBe(true);
  });
});
