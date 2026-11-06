/**
 * Nastavení → Kategorizace, pure parts: the order of the rule packs, which packs a switch leaves
 * enabled, what a custom rule may be offered, the payload of its on/off switch and the wording
 * parts of a learned payee. The rules are the desktop's (`RulePacksCard`, `CategorizationRules`).
 */
import type { CustomRule, CustomRuleInput, LearnedPayeeEntry, RulePackInfo } from '@shared/schema';
import { formatIbanDisplay } from '@/utils/rule-hits';

/** Enabled packs first, then `global`, then by the localized label (the desktop card's order). */
export function sortPacks(
  packs: readonly RulePackInfo[],
  label: (pack: RulePackInfo) => string,
  locale: string
): RulePackInfo[] {
  return [...packs].sort((a, b) => {
    if (a.enabled !== b.enabled) return a.enabled ? -1 : 1;
    if (a.packId === 'global') return -1;
    if (b.packId === 'global') return 1;
    return label(a).localeCompare(label(b), locale);
  });
}

/** The ids `setRulePacksEnabled` gets: the packs enabled on the server, with `packId` switched. */
export function nextEnabledPacks(
  packs: readonly RulePackInfo[],
  packId: string,
  enabled: boolean
): string[] {
  const ids = packs.filter((p) => p.enabled).map((p) => p.packId);
  return enabled
    ? [...ids.filter((id) => id !== packId), packId]
    : ids.filter((id) => id !== packId);
}

/** A system rule can be neither switched nor deleted (the desktop disables both). */
export function customRuleActions(rule: Pick<CustomRule, 'isSystem'>): {
  toggle: boolean;
  delete: boolean;
} {
  return { toggle: !rule.isSystem, delete: !rule.isSystem };
}

/** The update the switch sends: every field of the rule as it is, with `isActive` changed. */
export function customRuleInput(rule: CustomRule, isActive: boolean): CustomRuleInput {
  return {
    name: rule.name,
    ruleType: rule.ruleType,
    pattern: rule.pattern,
    categoryId: rule.categoryId,
    priority: rule.priority,
    isActive,
    stopProcessing: rule.stopProcessing,
    ibanPattern: rule.ibanPattern,
  };
}

/** What names a learned rule: the payee as it was written, else the normalized one, else the IBAN. */
export function learnedPayeeTitle(entry: LearnedPayeeEntry): string {
  return (
    entry.originalPayee || entry.normalizedPayee || formatIbanDisplay(entry.counterpartyIban) || '—'
  );
}

export type LearnedPayeeScope =
  | { kind: 'scoped'; account: string | null; direction: 'incoming' | 'outgoing' | null }
  | { kind: 'legacy' };

/**
 * Where a learned rule applies: one account and direction, or (an old rule without an account)
 * everywhere. A rule of a deleted account stays scoped, with no account name.
 */
export function learnedPayeeScope(
  entry: LearnedPayeeEntry,
  accountName: (id: string) => string | null
): LearnedPayeeScope {
  if (!entry.bankAccountId) return { kind: 'legacy' };
  return {
    kind: 'scoped',
    account: accountName(entry.bankAccountId),
    direction: entry.isCredit === null ? null : entry.isCredit ? 'incoming' : 'outgoing',
  };
}
