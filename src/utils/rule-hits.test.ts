import { describe, expect, it } from 'vitest';
import type { CustomRule, LearnedPayeeEntry, PackRuleInfo, RuleHitCount } from '@shared/schema';
import { formatIbanDisplay, hitMap, idleCount, maxHits, ruleTypeKey, topRule } from './rule-hits';

const hits: RuleHitCount[] = [
  { ruleId: 'c1', hits: 3, lastHit: 1 },
  { ruleId: 'l1', hits: 38, lastHit: 2 },
  { ruleId: 'p1', hits: 0, lastHit: null },
  { ruleId: 'unknown', hits: 99, lastHit: 3 },
];
const custom: CustomRule[] = [
  {
    id: 'c1',
    name: 'Výplata',
    ruleType: 'Contains',
    pattern: 'acme',
    categoryId: 'cat-income',
    priority: 80,
    isActive: true,
    stopProcessing: false,
    isSystem: false,
    createdAt: 0,
  },
];
const learned: LearnedPayeeEntry[] = [
  {
    id: 'l1',
    ruleType: 'payee_default',
    normalizedPayee: 'albert',
    originalPayee: 'ALBERT 0632',
    categoryId: 'cat-food',
    createdAt: 0,
    updatedAt: 0,
  },
];
const pack: PackRuleInfo[] = [
  {
    id: 'p1',
    name: 'Rohlík',
    ruleType: 'contains',
    pattern: 'rohlik',
    categoryId: 'cat-food',
    priority: 50,
    disabled: false,
  },
];

describe('rule hits', () => {
  it('finds the hardest-working rule among the rules it knows by name', () => {
    const top = topRule(hits, custom, learned, pack);
    expect(top).toMatchObject({ id: 'l1', hits: 38, source: 'learned', name: 'albert' });
    expect(topRule([], custom, learned, pack)).toBeNull();
  });

  it('scales bars to the largest count and counts idle rules', () => {
    const map = hitMap(hits);
    expect(maxHits(map, ['c1', 'p1'])).toBe(3);
    expect(maxHits(map, ['p1'])).toBe(1);
    expect(idleCount(map, ['c1', 'p1', 'missing'])).toBe(2);
  });

  it('maps rule type spellings and formats Czech IBANs as BBAN', () => {
    expect(ruleTypeKey('StartsWith')).toBe('starts_with');
    expect(ruleTypeKey('contains')).toBe('contains');
    expect(formatIbanDisplay('CZ65 0800 0000 1920 0014 5399')).toBe('19-2000145399/0800');
    expect(formatIbanDisplay('DE89370400440532013000')).toBe('DE89370400440532013000');
    expect(formatIbanDisplay(null)).toBe('');
  });
});
