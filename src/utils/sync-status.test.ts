import { describe, expect, it } from 'vitest';
import { backupFileName, secondsToDate } from './sync-status';

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
