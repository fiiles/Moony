import type { Milestone } from '@shared/schema';

const DAY = 86_400;

/** "Teď jednat": in the reminder window and asking for an action. */
export function isAttention(m: Milestone): boolean {
  return m.stage === 'now' && m.tone === 'action';
}

/** "Teď jednat" and "Brzy" (soon items and info items), each keeping the backend order. */
export function groupMilestones(list: readonly Milestone[]): {
  now: Milestone[];
  soon: Milestone[];
} {
  const now: Milestone[] = [];
  const soon: Milestone[] = [];
  for (const m of list) (isAttention(m) ? now : soon).push(m);
  return { now, soon };
}

/** The number shown in the top-bar indicator. */
export function attentionCount(list: readonly Milestone[]): number {
  return list.filter(isAttention).length;
}

/** The page a milestone row leads to. */
export function milestoneHref(m: Milestone): string {
  const id = encodeURIComponent(m.sourceId ?? '');
  switch (m.kind) {
    case 'insurance_anniversary':
    case 'insurance_end':
    case 'insurance_payment':
      return `/insurance/${id}`;
    case 'loan_fixation_end':
    case 'loan_fixation_expired':
    case 'loan_payoff':
    case 'loan_balance_check':
      return `/loans/${id}`;
    case 'bond_maturity':
    case 'bond_coupon':
      return '/bonds';
    case 'account_termination':
    case 'savings_rate_end':
      return `/bank-accounts/${id}`;
    case 'balances_stale':
      return '/bank-accounts';
    case 'valuation_stale':
      return `/real-estate/${id}`;
    case 'backup_stale':
      return '/settings/data';
    case 'watch_target':
      return `/stock-monitor/${id}`;
  }
}

/** Whole UTC days from `from` to `to` (unix seconds); negative when `to` is earlier. */
export function daysBetween(from: number, to: number): number {
  return Math.floor(to / DAY) - Math.floor(from / DAY);
}

/**
 * The day the row's relative label measures: the deadline while acting is still
 * possible, else the event, else the last backup / update / valuation.
 */
export function referenceDay(m: Milestone): number | null {
  if (m.actionDay !== null && m.tone === 'action') return m.actionDay;
  return m.dueDay ?? m.sinceDay;
}

export type RelativeKey = 'today' | 'tomorrow' | 'inDays' | 'inMonths' | 'daysAgo' | 'monthsAgo';

/** "dnes", "zítra", "za 38 dní", "za 5 měsíců", "před 41 dny", "před 13 měsíci". */
export function relativeLabel(days: number): { key: RelativeKey; count: number } {
  if (days === 0) return { key: 'today', count: 0 };
  if (days === 1) return { key: 'tomorrow', count: 1 };
  if (days < 0) {
    const ago = -days;
    return ago <= 60
      ? { key: 'daysAgo', count: ago }
      : { key: 'monthsAgo', count: Math.round(ago / 30) };
  }
  return days <= 60
    ? { key: 'inDays', count: days }
    : { key: 'inMonths', count: Math.round(days / 30) };
}
