import { describe, expect, it } from 'vitest';
import type { MenuPreferences } from '@shared/schema';
import { gettingStartedItems } from './getting-started';

const nothingDone = {
  hasTransactions: false,
  hasInvestment: false,
  hasBudget: false,
  lastBackupAt: null,
};

const prefs = (over: Partial<MenuPreferences> = {}): MenuPreferences => ({
  loans: true,
  insurance: true,
  investments: true,
  bonds: true,
  realEstate: true,
  crypto: true,
  otherAssets: true,
  ...over,
});

describe('gettingStartedItems', () => {
  it('lists every step when all areas are on, with the core steps not optional', () => {
    const items = gettingStartedItems(nothingDone, prefs());
    expect(items.map((i) => [i.key, i.optional])).toEqual([
      ['importStatement', false],
      ['addInvestment', true],
      ['setBudget', true],
      ['makeBackup', false],
    ]);
  });

  it('leaves out the investment step when investments and crypto are off', () => {
    const items = gettingStartedItems(
      nothingDone,
      prefs({ investments: false, bonds: false, crypto: false })
    );
    expect(items.map((i) => i.key)).toEqual(['importStatement', 'setBudget', 'makeBackup']);
  });

  it('keeps the investment step for crypto only and links to it', () => {
    const items = gettingStartedItems(nothingDone, prefs({ investments: false, bonds: false }));
    expect(items.find((i) => i.key === 'addInvestment')?.href).toBe('/crypto');
  });

  it('treats a missing crypto flag as shown and missing preferences as everything on', () => {
    const { crypto: _crypto, ...withoutCrypto } = prefs({ investments: false });
    expect(
      gettingStartedItems(nothingDone, withoutCrypto).some((i) => i.key === 'addInvestment')
    ).toBe(true);
    expect(gettingStartedItems(nothingDone, undefined)).toHaveLength(4);
  });

  it('marks steps done from the progress', () => {
    const items = gettingStartedItems(
      { hasTransactions: true, hasInvestment: false, hasBudget: true, lastBackupAt: 1_700_000_000 },
      prefs()
    );
    expect(items.filter((i) => i.done).map((i) => i.key)).toEqual([
      'importStatement',
      'setBudget',
      'makeBackup',
    ]);
  });
});
