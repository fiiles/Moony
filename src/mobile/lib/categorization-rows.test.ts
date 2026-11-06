import { describe, expect, it } from 'vitest';
import type { CustomRule, LearnedPayeeEntry, RulePackInfo } from '@shared/schema';
import {
  customRuleActions,
  customRuleInput,
  learnedPayeeScope,
  learnedPayeeTitle,
  nextEnabledPacks,
  learnedSearchShown,
  sortPacks,
  splitPacks,
} from './categorization-rows';

const pack = (packId: string, enabled: boolean): RulePackInfo => ({
  packId,
  country: packId.toUpperCase(),
  version: 1,
  ruleCount: 3,
  enabled,
});

const rule = (over: Partial<CustomRule> = {}): CustomRule => ({
  id: 'r1',
  name: 'Lidl',
  ruleType: 'contains',
  pattern: 'lidl',
  categoryId: 'groceries',
  priority: 5,
  isActive: true,
  stopProcessing: true,
  isSystem: false,
  createdAt: 0,
  ibanPattern: 'CZ6508000000192000145399',
  ...over,
});

const learned = (over: Partial<LearnedPayeeEntry> = {}): LearnedPayeeEntry => ({
  id: 'l1',
  bankAccountId: 'a1',
  isCredit: false,
  ruleType: 'payee_default',
  normalizedPayee: 'lidl',
  originalPayee: 'LIDL PRAHA 4',
  counterpartyIban: null,
  categoryId: 'groceries',
  createdAt: 0,
  updatedAt: 0,
  ...over,
});

describe('sortPacks', () => {
  const label = (p: RulePackInfo) => p.country;
  it('puts an enabled pack before a disabled one', () => {
    const sorted = sortPacks([pack('at', false), pack('cz', true)], label, 'cs');
    expect(sorted.map((p) => p.packId)).toEqual(['cz', 'at']);
  });
  it('puts global first among the enabled, the rest by label', () => {
    const sorted = sortPacks(
      [pack('sk', true), pack('global', true), pack('cz', true), pack('at', false)],
      label,
      'cs'
    );
    expect(sorted.map((p) => p.packId)).toEqual(['global', 'cz', 'sk', 'at']);
  });
});

describe('nextEnabledPacks', () => {
  const packs = [pack('cz', true), pack('sk', false), pack('global', true)];
  it('adds the pack when enabling and keeps the others', () => {
    expect(nextEnabledPacks(packs, 'sk', true)).toEqual(['cz', 'global', 'sk']);
  });
  it('removes only the pack when disabling', () => {
    expect(nextEnabledPacks(packs, 'cz', false)).toEqual(['global']);
  });
});

describe('customRuleActions', () => {
  it('offers neither toggle nor delete on a system rule', () => {
    expect(customRuleActions(rule({ isSystem: true }))).toEqual({ toggle: false, delete: false });
  });
  it('offers both on the own rules', () => {
    expect(customRuleActions(rule())).toEqual({ toggle: true, delete: true });
  });
});

describe('customRuleInput', () => {
  it('copies every field and sets isActive', () => {
    expect(customRuleInput(rule(), false)).toEqual({
      name: 'Lidl',
      ruleType: 'contains',
      pattern: 'lidl',
      categoryId: 'groceries',
      priority: 5,
      isActive: false,
      stopProcessing: true,
      ibanPattern: 'CZ6508000000192000145399',
    });
  });
});

describe('learnedPayeeTitle', () => {
  it('prefers the normalized payee, then the original one (as the desktop)', () => {
    expect(learnedPayeeTitle(learned())).toBe('lidl');
    expect(learnedPayeeTitle(learned({ normalizedPayee: null }))).toBe('LIDL PRAHA 4');
  });
  it('shows the formatted IBAN of an IBAN-only entry', () => {
    const entry = learned({
      originalPayee: null,
      normalizedPayee: null,
      counterpartyIban: 'CZ6508000000192000145399',
    });
    expect(learnedPayeeTitle(entry)).toBe('19-2000145399/0800');
  });
  it('falls back to a dash with nothing at all', () => {
    expect(learnedPayeeTitle(learned({ originalPayee: null, normalizedPayee: null }))).toBe('—');
  });
});

describe('learnedPayeeScope', () => {
  const names = (id: string) => (id === 'a1' ? 'Běžný' : null);
  it('names the account and the direction', () => {
    expect(learnedPayeeScope(learned({ isCredit: true }), names)).toEqual({
      kind: 'scoped',
      account: 'Běžný',
      direction: 'incoming',
    });
    expect(learnedPayeeScope(learned(), names)).toEqual({
      kind: 'scoped',
      account: 'Běžný',
      direction: 'outgoing',
    });
  });
  it('keeps a rule of a deleted account, without a name', () => {
    expect(learnedPayeeScope(learned({ bankAccountId: 'gone' }), names)).toEqual({
      kind: 'scoped',
      account: null,
      direction: 'outgoing',
    });
  });
  it('is legacy without an account', () => {
    expect(learnedPayeeScope(learned({ bankAccountId: null, isCredit: null }), names)).toEqual({
      kind: 'legacy',
    });
  });
  it('has no direction when the rule has none', () => {
    expect(learnedPayeeScope(learned({ isCredit: null }), names)).toMatchObject({
      direction: null,
    });
  });
});

describe('splitPacks', () => {
  it('keeps the enabled packs inline and the rest for the other-countries sheet', () => {
    const { enabled, other } = splitPacks([
      pack('global', true),
      pack('cz', false),
      pack('de', true),
    ]);
    expect(enabled.map((p) => p.packId)).toEqual(['global', 'de']);
    expect(other.map((p) => p.packId)).toEqual(['cz']);
  });
  it('leaves the inline part empty when no pack is enabled', () => {
    const { enabled, other } = splitPacks([pack('cz', false)]);
    expect(enabled).toEqual([]);
    expect(other).toHaveLength(1);
  });
});

describe('learnedSearchShown', () => {
  it('shows the field above the threshold', () => {
    expect(learnedSearchShown(16, '', 15)).toBe(true);
    expect(learnedSearchShown(15, '', 15)).toBe(false);
  });
  it('keeps the field while a query is typed, so the filter can always be cleared', () => {
    expect(learnedSearchShown(15, 'lidl', 15)).toBe(true);
    expect(learnedSearchShown(3, '  ', 15)).toBe(false);
  });
});
