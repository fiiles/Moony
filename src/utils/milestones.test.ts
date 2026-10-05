import { describe, expect, it } from 'vitest';
import type { TFunction } from 'i18next';
import type { Milestone, MilestoneKind } from '@shared/schema';
import {
  MILESTONE_GROUP_IDS,
  agendaDay,
  columns,
  daysBetween,
  groupOf,
  isUrgent,
  kindsOf,
  milestoneHref,
  relativeLabel,
  splitMilestones,
  upcomingDeadlines,
  upkeepShort,
} from './milestones';

const DAY = 86_400;
const TODAY = 20_000 * DAY;

function m(overrides: Partial<Milestone>): Milestone {
  return {
    key: 'k',
    kind: 'insurance_end',
    stage: 'now',
    tone: 'action',
    sourceId: 'p1',
    title: 'Pojistka',
    dueDay: null,
    actionDay: null,
    sinceDay: null,
    amount: null,
    currency: null,
    referenceAmount: null,
    direction: null,
    count: null,
    ...overrides,
  };
}

const ALL_KINDS: MilestoneKind[] = [
  'insurance_anniversary',
  'insurance_end',
  'insurance_payment',
  'loan_fixation_end',
  'loan_fixation_expired',
  'loan_payoff',
  'loan_balance_check',
  'bond_maturity',
  'bond_coupon',
  'account_termination',
  'savings_rate_end',
  'balances_stale',
  'backup_stale',
  'valuation_stale',
  'watch_target',
];

describe('agendaDay', () => {
  it('is the notice deadline while it can be met, else the event, and today for a target', () => {
    const anniversary = m({
      kind: 'insurance_anniversary',
      dueDay: TODAY + 66 * DAY,
      actionDay: TODAY + 23 * DAY,
    });
    expect(agendaDay(anniversary, TODAY)).toBe(TODAY + 23 * DAY);
    expect(agendaDay({ ...anniversary, tone: 'info' }, TODAY)).toBe(TODAY + 66 * DAY);
    expect(agendaDay(m({ kind: 'watch_target' }), TODAY)).toBe(TODAY);
  });
});

describe('splitMilestones', () => {
  it('sorts the agenda by its day and folds upkeep apart, keeping the backend order of upkeep', () => {
    const payment = m({
      key: 'pay',
      kind: 'insurance_payment',
      tone: 'action',
      dueDay: TODAY + 60 * DAY,
    });
    const deadline = m({ key: 'end', kind: 'insurance_end', dueDay: TODAY + 10 * DAY });
    const target = m({ key: 'aapl', kind: 'watch_target' });
    const backup = m({ key: 'b', kind: 'backup_stale', sinceDay: TODAY - 40 * DAY });
    const valuation = m({ key: 'v', kind: 'valuation_stale', sinceDay: TODAY - 400 * DAY });
    const { agenda, upkeep } = splitMilestones(
      [payment, backup, deadline, valuation, target],
      TODAY
    );
    expect(agenda.map((x) => x.key)).toEqual(['aapl', 'end', 'pay']);
    expect(upkeep.map((x) => x.key)).toEqual(['b', 'v']);
  });
});

describe('upcomingDeadlines and isUrgent', () => {
  it('keeps deadlines up to 14 days ahead, never information, upkeep or targets', () => {
    const soon = m({ key: 'rate', kind: 'savings_rate_end', dueDay: TODAY + 10 * DAY });
    const later = m({ key: 'fix', kind: 'loan_fixation_end', dueDay: TODAY + 15 * DAY });
    const info = m({ key: 'coupon', kind: 'bond_coupon', tone: 'info', dueDay: TODAY + 3 * DAY });
    const passed = m({
      key: 'ann',
      kind: 'insurance_anniversary',
      tone: 'info',
      dueDay: TODAY + 5 * DAY,
      actionDay: TODAY - 1 * DAY,
    });
    const target = m({ key: 't', kind: 'watch_target' });
    const backup = m({ key: 'b', kind: 'backup_stale' });
    const first = m({ key: 'end', kind: 'insurance_end', dueDay: TODAY + 2 * DAY });
    expect(
      upcomingDeadlines([soon, later, info, passed, target, backup, first], TODAY).map((x) => x.key)
    ).toEqual(['end', 'rate']);
    expect(isUrgent(first, TODAY)).toBe(true);
    expect(isUrgent(soon, TODAY)).toBe(false);
    expect(isUrgent(info, TODAY)).toBe(false);
  });
});

describe('upcomingDeadlines and isUrgent edges', () => {
  it('includes a deadline exactly 14 days ahead and excludes one a day in the past', () => {
    const edge = m({ key: 'edge', kind: 'insurance_end', dueDay: TODAY + 14 * DAY });
    const overdue = m({ key: 'late', kind: 'insurance_end', dueDay: TODAY - 1 * DAY });
    const today = m({ key: 'today', kind: 'insurance_end', dueDay: TODAY });
    expect(upcomingDeadlines([edge, overdue, today], TODAY).map((x) => x.key)).toEqual([
      'today',
      'edge',
    ]);
  });

  it('is urgent at 7 days and not at 8', () => {
    expect(isUrgent(m({ kind: 'insurance_end', dueDay: TODAY + 7 * DAY }), TODAY)).toBe(true);
    expect(isUrgent(m({ kind: 'insurance_end', dueDay: TODAY + 8 * DAY }), TODAY)).toBe(false);
  });
});

describe('groups', () => {
  it('every kind belongs to exactly one group', () => {
    const grouped = MILESTONE_GROUP_IDS.flatMap((g) => kindsOf(g));
    expect([...grouped].sort()).toEqual([...ALL_KINDS].sort());
    expect(groupOf('loan_fixation_expired')).toBe('loanChecks');
    expect(groupOf('insurance_payment')).toBe('insurancePayments');
  });
});

describe('columns', () => {
  it('fills the first column first', () => {
    expect(columns([1, 2, 3, 4, 5])).toEqual([
      [1, 2, 3],
      [4, 5],
    ]);
    expect(columns([])).toEqual([[], []]);
  });
});

describe('milestoneHref', () => {
  it('links each kind to its page', () => {
    expect(milestoneHref(m({ kind: 'insurance_payment', sourceId: 'p1' }))).toBe('/insurance/p1');
    expect(milestoneHref(m({ kind: 'loan_balance_check', sourceId: 'l1' }))).toBe('/loans/l1');
    expect(milestoneHref(m({ kind: 'bond_coupon', sourceId: 'b1' }))).toBe('/bonds');
    expect(milestoneHref(m({ kind: 'savings_rate_end', sourceId: 'a1' }))).toBe(
      '/bank-accounts/a1'
    );
    expect(milestoneHref(m({ kind: 'balances_stale', sourceId: null }))).toBe('/bank-accounts');
    expect(milestoneHref(m({ kind: 'valuation_stale', sourceId: 'r1' }))).toBe('/real-estate/r1');
    expect(milestoneHref(m({ kind: 'backup_stale', sourceId: null }))).toBe('/settings/data');
    expect(milestoneHref(m({ kind: 'watch_target', sourceId: 'BRK.B' }))).toBe(
      '/stock-monitor/BRK.B'
    );
  });
});

describe('daysBetween and relativeLabel', () => {
  it('counts whole UTC days', () => {
    expect(daysBetween(10 * DAY + 3_600, 12 * DAY)).toBe(2);
    expect(daysBetween(12 * DAY, 10 * DAY)).toBe(-2);
  });
  it('labels today, tomorrow, days, months and the past', () => {
    expect(relativeLabel(0)).toEqual({ key: 'today', count: 0 });
    expect(relativeLabel(1)).toEqual({ key: 'tomorrow', count: 1 });
    expect(relativeLabel(38)).toEqual({ key: 'inDays', count: 38 });
    expect(relativeLabel(150)).toEqual({ key: 'inMonths', count: 5 });
    expect(relativeLabel(-41)).toEqual({ key: 'daysAgo', count: 41 });
    expect(relativeLabel(-400)).toEqual({ key: 'monthsAgo', count: 13 });
  });
});

describe('upkeepShort', () => {
  /** A stand-in for t() that echoes the key and its interpolation values. */
  const t = ((key: string, options?: Record<string, unknown>) =>
    `${key}|${JSON.stringify(options)}`) as unknown as TFunction<'milestones'>;

  it('counts accounts for stale balances and names the item for the rest', () => {
    expect(upkeepShort(m({ kind: 'balances_stale', title: 'Účty', count: 3 }), t)).toBe(
      'short.balances_stale|{"count":3}'
    );
    expect(upkeepShort(m({ kind: 'balances_stale', count: null }), t)).toBe(
      'short.balances_stale|{"count":0}'
    );
    expect(upkeepShort(m({ kind: 'valuation_stale', title: 'Byt 3+kk' }), t)).toBe(
      'short.valuation_stale|{"title":"Byt 3+kk"}'
    );
    expect(upkeepShort(m({ kind: 'backup_stale', title: '' }), t)).toBe(
      'short.backup_stale|{"title":""}'
    );
  });
});
