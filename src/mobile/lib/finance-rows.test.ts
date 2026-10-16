import { describe, expect, it } from 'vitest';
import { accountSub, accountTrail, cashChange, sortAccounts, STALE_DAYS } from './finance-rows';
const DAY = 86_400;
const TODAY = Date.UTC(2026, 9, 16) / 1000;

describe('accountSub', () => {
  const fio = { institution: { name: 'Fio' }, currency: 'CZK' };
  it('names the bank, the currency and the newest statement', () => {
    expect(accountSub({ ...fio, balanceAsOf: TODAY - 20 * DAY }, TODAY - 6 * DAY)).toEqual({
      institution: 'Fio',
      currency: 'CZK',
      stamp: { kind: 'statement', day: TODAY - 6 * DAY },
      manual: false,
    });
  });
  it('falls back to the balance date and to no institution', () => {
    expect(
      accountSub({ institution: null, currency: 'CZK', balanceAsOf: TODAY - 44 * DAY }, null)
    ).toEqual({
      institution: null,
      currency: 'CZK',
      stamp: { kind: 'balance', day: TODAY - 44 * DAY },
      manual: true,
    });
    expect(
      accountSub({ institution: null, currency: '', balanceAsOf: null }, null).stamp
    ).toBeNull();
  });
  it('stands on the balance date when that is newer than the newest import', () => {
    expect(accountSub({ ...fio, balanceAsOf: TODAY - DAY }, TODAY - 40 * DAY).stamp).toEqual({
      kind: 'balance',
      day: TODAY - DAY,
    });
  });
  it('stands on the statement when the import is newer, and on a tie', () => {
    expect(accountSub({ ...fio, balanceAsOf: TODAY - 40 * DAY }, TODAY - DAY).stamp).toEqual({
      kind: 'statement',
      day: TODAY - DAY,
    });
    // The same UTC day, an instant apart: still a tie
    expect(
      accountSub({ ...fio, balanceAsOf: TODAY - 3 * DAY }, TODAY - 3 * DAY + 3600).stamp
    ).toEqual({
      kind: 'statement',
      day: TODAY - 3 * DAY + 3600,
    });
    expect(accountSub({ ...fio, balanceAsOf: TODAY - 3 * DAY }, TODAY - 3 * DAY).stamp).toEqual({
      kind: 'statement',
      day: TODAY - 3 * DAY,
    });
  });
  it('stands on the statement when the account has no balance date', () => {
    expect(accountSub({ ...fio, balanceAsOf: null }, TODAY - 2 * DAY).stamp).toEqual({
      kind: 'statement',
      day: TODAY - 2 * DAY,
    });
  });
  it('is manual only without an institution and without an import', () => {
    const none = { institution: null, currency: 'CZK', balanceAsOf: TODAY - DAY };
    expect(accountSub(none, null).manual).toBe(true);
    // Imported statements make it a bank account in all but name
    expect(accountSub(none, TODAY - 9 * DAY).manual).toBe(false);
    expect(accountSub({ ...fio, balanceAsOf: TODAY - DAY }, null).manual).toBe(false);
    expect(accountSub({ ...fio, balanceAsOf: TODAY - DAY }, TODAY - 9 * DAY).manual).toBe(false);
  });
});

describe('accountTrail', () => {
  const base = {
    currency: 'CZK',
    mainCurrency: 'CZK',
    todayDay: TODAY,
    converted: 86_420,
    stamp: null,
    last30: undefined,
  };
  it('is the 30-day change when the account moved', () => {
    expect(accountTrail({ ...base, last30: { net: 9_870, count: 12 } })).toEqual({
      kind: 'change',
      net: 9_870,
    });
  });
  it('is the main-currency equivalent for a foreign account without movements', () => {
    expect(accountTrail({ ...base, currency: 'EUR', converted: 30_660 })).toEqual({
      kind: 'converted',
      value: 30_660,
    });
    expect(accountTrail({ ...base, currency: 'CHF', converted: null })).toEqual({
      kind: 'converted',
      value: null,
    });
  });
  it('says how old a stale statement or balance is', () => {
    expect(accountTrail({ ...base, stamp: { kind: 'statement', day: TODAY - 38 * DAY } })).toEqual({
      kind: 'stale',
      what: 'statement',
      days: 38,
    });
    expect(
      accountTrail({ ...base, stamp: { kind: 'balance', day: TODAY - STALE_DAYS * DAY } })
    ).toEqual({ kind: 'none' });
  });
  it('is nothing for a fresh main-currency account without movements', () => {
    expect(accountTrail({ ...base, stamp: { kind: 'statement', day: TODAY - 3 * DAY } })).toEqual({
      kind: 'none',
    });
  });
});

describe('cashChange', () => {
  it('relates the 30-day net to the balance before it', () => {
    expect(cashChange(214_380, 12_400)).toEqual({ delta: 12_400, ratio: 12_400 / 201_980 });
  });
  it('is null when there is no base', () => {
    expect(cashChange(0, 0)).toBeNull();
    expect(cashChange(500, 500)).toBeNull();
  });
  it('is null when nothing moved', () => {
    expect(cashChange(500, 0)).toBeNull();
  });
  it('relates a drop to the balance before it', () => {
    expect(cashChange(100, -50)).toEqual({ delta: -50, ratio: -50 / 150 });
  });
});

describe('sortAccounts', () => {
  const a = { id: 'a', name: 'Běžný', currency: 'CZK', balance: '100' };
  const b = { id: 'b', name: 'Alfa', currency: 'EUR', balance: '10' };
  const convert = (amount: number, currency: string) => (currency === 'EUR' ? amount * 25 : amount);
  const activity = (id: string) =>
    id === 'a' ? { net: -5, lastDay: TODAY } : { net: 40, lastDay: TODAY - DAY };
  it('sorts by converted balance descending, name ascending, change and last movement descending', () => {
    expect(sortAccounts([a, b], 'balance', { convert, activity }).map((x) => x.id)).toEqual([
      'b',
      'a',
    ]);
    expect(sortAccounts([a, b], 'name', { convert, activity }).map((x) => x.id)).toEqual([
      'b',
      'a',
    ]);
    expect(sortAccounts([a, b], 'change', { convert, activity }).map((x) => x.id)).toEqual([
      'b',
      'a',
    ]);
    expect(sortAccounts([a, b], 'last', { convert, activity }).map((x) => x.id)).toEqual([
      'a',
      'b',
    ]);
  });
  it('puts an account without a rate last', () => {
    const noRate = (amount: number, currency: string) => (currency === 'EUR' ? null : amount);
    expect(sortAccounts([b, a], 'balance', { convert: noRate, activity }).map((x) => x.id)).toEqual(
      ['a', 'b']
    );
  });
});
