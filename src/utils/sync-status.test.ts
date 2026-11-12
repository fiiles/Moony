import { describe, expect, it } from 'vitest';
import { backupFileName, isSelfHostedPaidUntil, secondsToDate } from './sync-status';

describe('isSelfHostedPaidUntil', () => {
  it('is true from the year 9000 on', () => {
    expect(isSelfHostedPaidUntil(253402300799)).toBe(true); // 9999-12-31
    expect(isSelfHostedPaidUntil(221845392000)).toBe(true); // 1 Jan 9000
  });
  it('is false for an ordinary subscription end, none, or a date just before year 9000', () => {
    expect(isSelfHostedPaidUntil(1_800_000_000)).toBe(false);
    expect(isSelfHostedPaidUntil(null)).toBe(false);
    expect(isSelfHostedPaidUntil(221845391999)).toBe(false);
  });
});

describe('backupFileName', () => {
  it('takes the last path part with either separator', () => {
    expect(backupFileName('/Users/a/Moony/backup-1.zip')).toBe('backup-1.zip');
    expect(backupFileName('C:\\Users\\a\\backup-2.zip')).toBe('backup-2.zip');
    expect(backupFileName('plain.zip')).toBe('plain.zip');
  });
});

describe('secondsToDate', () => {
  it('makes a date for an ordinary or year-9999 value', () => {
    expect(secondsToDate(1_800_000_000)?.getUTCFullYear()).toBe(2027);
    expect(secondsToDate(253402300799)?.getUTCFullYear()).toBe(9999);
  });
  it('is null for none, negative, non-finite or beyond year 9999', () => {
    expect(secondsToDate(null)).toBeNull();
    expect(secondsToDate(-1)).toBeNull();
    expect(secondsToDate(Number.NaN)).toBeNull();
    expect(secondsToDate(3e15)).toBeNull();
  });
});
