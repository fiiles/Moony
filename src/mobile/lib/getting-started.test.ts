import { describe, expect, it } from 'vitest';
import { addAccountPath, opensAddAccount, phoneGettingStartedItems } from './getting-started';

const progress = {
  hasTransactions: false,
  hasInvestment: false,
  hasBudget: false,
  lastBackupAt: null,
};

describe('phoneGettingStartedItems', () => {
  it('keeps the desktop items and sends each to its phone page', () => {
    expect(phoneGettingStartedItems(progress, undefined).map((i) => [i.key, i.href])).toEqual([
      ['importStatement', '/finance'],
      ['addInvestment', '/wealth/stocks'],
      ['setBudget', '/cashflow/budgets'],
      ['makeBackup', '/settings/backup'],
    ]);
  });

  it('sends the investment to Krypto when stocks are not tracked', () => {
    const items = phoneGettingStartedItems(progress, {
      investments: false,
      crypto: true,
    } as never);
    expect(items.find((i) => i.key === 'addInvestment')?.href).toBe('/wealth/crypto');
  });

  it('leaves the investment out when neither is tracked, and keeps the done flags', () => {
    const items = phoneGettingStartedItems(
      { ...progress, hasTransactions: true, lastBackupAt: 1 },
      { investments: false, crypto: false } as never
    );
    expect(items.map((i) => [i.key, i.done])).toEqual([
      ['importStatement', true],
      ['setBudget', false],
      ['makeBackup', true],
    ]);
  });
});

describe('phoneGettingStartedItems on a device that cannot back up', () => {
  it('has no backup step', () => {
    const keys = phoneGettingStartedItems(progress, undefined, false).map((i) => i.key);
    expect(keys).toEqual(['importStatement', 'addInvestment', 'setBudget']);
  });
});

describe('opensAddAccount', () => {
  it("opens the account form for the empty Přehled's path only", () => {
    expect(opensAddAccount(addAccountPath.slice(addAccountPath.indexOf('?')))).toBe(true);
    expect(opensAddAccount('?add=loan')).toBe(false);
    expect(opensAddAccount('')).toBe(false);
  });

  it('leads "Protect your data" to Záloha a přenos and drops it where backups cannot be made', () => {
    const sync = { featureOn: true, syncOn: false };
    const items = phoneGettingStartedItems(progress, undefined, true, sync);
    expect(items.find((i) => i.key === 'protectData')?.href).toBe('/settings/backup');
    expect(items.some((i) => i.key === 'makeBackup')).toBe(false);
    const noBackup = phoneGettingStartedItems(progress, undefined, false, sync);
    expect(noBackup.some((i) => i.key === 'protectData' || i.key === 'makeBackup')).toBe(false);
  });
});
