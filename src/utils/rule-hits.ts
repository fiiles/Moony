import type { CustomRule, LearnedPayeeEntry, PackRuleInfo, RuleHitCount } from '@shared/schema';

/** The window the rules page measures hits over (prototype: 90 days). */
export const HIT_DAYS = 90;

/** Hits by rule id, for O(1) lookups in the tables. */
export function hitMap(counts: readonly RuleHitCount[]): Map<string, RuleHitCount> {
  return new Map(counts.map((c) => [c.ruleId, c]));
}

/** The largest hit count among `ids` (the bar's 100 %), at least 1. */
export function maxHits(hits: Map<string, RuleHitCount>, ids: readonly string[]): number {
  let max = 0;
  for (const id of ids) max = Math.max(max, hits.get(id)?.hits ?? 0);
  return Math.max(1, max);
}

/** How many of `ids` had no hit. */
export function idleCount(hits: Map<string, RuleHitCount>, ids: readonly string[]): number {
  return ids.filter((id) => (hits.get(id)?.hits ?? 0) === 0).length;
}

export type TopRuleSource = 'custom' | 'learned' | 'pack';

export interface TopRule {
  id: string;
  hits: number;
  source: TopRuleSource;
  /** Rule name, or the learned counterparty. */
  name: string;
  categoryId: string;
}

/**
 * The hardest-working rule among the rules the page knows by name (custom,
 * learned, and the rules of the pack currently shown). Null without any hit.
 */
export function topRule(
  hits: readonly RuleHitCount[],
  custom: readonly CustomRule[],
  learned: readonly LearnedPayeeEntry[],
  pack: readonly PackRuleInfo[]
): TopRule | null {
  const byId = new Map<string, Omit<TopRule, 'hits'>>();
  for (const r of custom) {
    byId.set(r.id, { id: r.id, source: 'custom', name: r.name, categoryId: r.categoryId });
  }
  for (const r of learned) {
    byId.set(r.id, {
      id: r.id,
      source: 'learned',
      name: r.normalizedPayee || r.originalPayee || r.counterpartyIban || r.id,
      categoryId: r.categoryId,
    });
  }
  for (const r of pack) {
    byId.set(r.id, { id: r.id, source: 'pack', name: r.name, categoryId: r.categoryId });
  }
  let best: TopRule | null = null;
  for (const c of hits) {
    if (c.hits === 0) continue;
    const known = byId.get(c.ruleId);
    if (!known) continue;
    if (!best || c.hits > best.hits) best = { ...known, hits: c.hits };
  }
  return best;
}

/** Lower-case rule type key for the `ruleTypes.*` labels (`Contains` → `contains`). */
export function ruleTypeKey(ruleType: string): string {
  const key = ruleType.trim().toLowerCase();
  switch (key) {
    case 'startswith':
      return 'starts_with';
    case 'endswith':
      return 'ends_with';
    default:
      return key;
  }
}

/** IBAN as the Czech BBAN ("19-2817…/0800") when it is a Czech IBAN, else as is. */
export function formatIbanDisplay(iban: string | undefined | null): string {
  if (!iban) return '';
  const normalized = iban.replace(/\s/g, '').toUpperCase();
  if (normalized.startsWith('CZ') && normalized.length === 24) {
    const bankCode = normalized.slice(4, 8);
    const prefix = normalized.slice(8, 14).replace(/^0+/, '');
    const accountNumber = normalized.slice(14).replace(/^0+/, '');
    return prefix ? `${prefix}-${accountNumber}/${bankCode}` : `${accountNumber}/${bankCode}`;
  }
  return iban;
}

/**
 * The pack's country in the UI language ("Česko" for `cz`): packs are keyed by
 * ISO country code, so `Intl.DisplayNames` knows them; `global` keeps its name.
 */
export function packCountryName(packId: string, fallback: string, locale: string): string {
  if (packId.length !== 2) return fallback;
  try {
    return new Intl.DisplayNames([locale], { type: 'region' }).of(packId.toUpperCase()) ?? fallback;
  } catch {
    return fallback;
  }
}
