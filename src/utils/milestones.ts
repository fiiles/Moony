import type { Milestone, MilestoneKind } from '@shared/schema';

const DAY = 86_400;

/** Undated data upkeep: folded into one line under the agenda, never in the top bar. */
export const UPKEEP_KINDS: readonly MilestoneKind[] = [
  'backup_stale',
  'balances_stale',
  'valuation_stale',
  'loan_balance_check',
  'loan_fixation_expired',
];

/** Dates the user can miss: the only kinds the top bar shows. */
export const DEADLINE_KINDS: readonly MilestoneKind[] = [
  'insurance_anniversary',
  'insurance_end',
  'loan_fixation_end',
  'bond_maturity',
  'account_termination',
  'savings_rate_end',
];

export type MilestoneGroupId =
  | 'insuranceDates'
  | 'insurancePayments'
  | 'loans'
  | 'bonds'
  | 'accounts'
  | 'targets'
  | 'backup'
  | 'balances'
  | 'valuations'
  | 'loanChecks';

/** "Nepřipomínat …" and the settings switches mute whole groups of kinds. */
const GROUP_OF: Record<MilestoneKind, MilestoneGroupId> = {
  insurance_anniversary: 'insuranceDates',
  insurance_end: 'insuranceDates',
  insurance_payment: 'insurancePayments',
  loan_fixation_end: 'loans',
  loan_payoff: 'loans',
  bond_maturity: 'bonds',
  bond_coupon: 'bonds',
  account_termination: 'accounts',
  savings_rate_end: 'accounts',
  watch_target: 'targets',
  backup_stale: 'backup',
  balances_stale: 'balances',
  valuation_stale: 'valuations',
  loan_balance_check: 'loanChecks',
  loan_fixation_expired: 'loanChecks',
};

/** Groups in settings order. */
export const MILESTONE_GROUP_IDS: readonly MilestoneGroupId[] = [
  'insuranceDates',
  'insurancePayments',
  'loans',
  'bonds',
  'accounts',
  'targets',
  'backup',
  'balances',
  'valuations',
  'loanChecks',
];

export function groupOf(kind: MilestoneKind): MilestoneGroupId {
  return GROUP_OF[kind];
}

export function kindsOf(group: MilestoneGroupId): MilestoneKind[] {
  return (Object.keys(GROUP_OF) as MilestoneKind[]).filter((kind) => GROUP_OF[kind] === group);
}

export function isUpkeep(m: Milestone): boolean {
  return UPKEEP_KINDS.includes(m.kind);
}

/** A deadline that can still be met (an anniversary past its notice deadline is information). */
export function isDeadline(m: Milestone): boolean {
  return DEADLINE_KINDS.includes(m.kind) && m.tone === 'action';
}

/**
 * The day an agenda row shows: the notice deadline while it can be met, otherwise the
 * event; a crossed target is today.
 */
export function agendaDay(m: Milestone, today: number): number {
  if (m.kind === 'watch_target') return today;
  if (m.actionDay !== null && m.tone === 'action') return m.actionDay;
  return m.dueDay ?? today;
}

/** Agenda (sorted by its day; ties keep the backend order) and data upkeep. */
export function splitMilestones(
  list: readonly Milestone[],
  today: number
): { agenda: Milestone[]; upkeep: Milestone[] } {
  const agenda = list.filter((m) => !isUpkeep(m));
  const upkeep = list.filter(isUpkeep);
  agenda.sort((a, b) => agendaDay(a, today) - agendaDay(b, today));
  return { agenda, upkeep };
}

/** A deadline at most 7 days away: the only rows with a dark date chip. */
export function isUrgent(m: Milestone, today: number): boolean {
  return isDeadline(m) && daysBetween(today, agendaDay(m, today)) <= 7;
}

/** Deadlines from today up to `withinDays` ahead, nearest first (the top bar). */
export function upcomingDeadlines(
  list: readonly Milestone[],
  today: number,
  withinDays = 14
): Milestone[] {
  return list
    .filter((m) => {
      if (!isDeadline(m)) return false;
      const days = daysBetween(today, agendaDay(m, today));
      return days >= 0 && days <= withinDays;
    })
    .sort((a, b) => agendaDay(a, today) - agendaDay(b, today));
}

/** Two columns filled top to bottom, the first one first. */
export function columns<T>(items: readonly T[]): [T[], T[]] {
  const half = Math.ceil(items.length / 2);
  return [items.slice(0, half), items.slice(half)];
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
