import { describe, expect, it } from 'vitest';
import type { Milestone } from '@shared/schema';
import {
  attentionCount,
  daysBetween,
  groupMilestones,
  milestoneHref,
  referenceDay,
  relativeLabel,
} from './milestones';

const DAY = 86_400;

function m(overrides: Partial<Milestone>): Milestone {
  return {
    key: 'k',
    kind: 'insurance_anniversary',
    stage: 'now',
    tone: 'action',
    sourceId: 'p1',
    title: 'Úrazová pojistka',
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

describe('groupMilestones', () => {
  it('puts now + action items under "act now" and everything else under "soon", keeping order', () => {
    const a = m({ key: 'a' });
    const b = m({ key: 'b', stage: 'soon' });
    const c = m({ key: 'c', tone: 'info' });
    const d = m({ key: 'd' });
    const { now, soon } = groupMilestones([a, b, c, d]);
    expect(now.map((x) => x.key)).toEqual(['a', 'd']);
    expect(soon.map((x) => x.key)).toEqual(['b', 'c']);
    expect(attentionCount([a, b, c, d])).toBe(2);
  });

  it('orders "soon" by date even when a now+info item comes earlier from the backend', () => {
    const payoff = m({ key: 'payoff', kind: 'loan_payoff', tone: 'info', dueDay: 25 * DAY });
    const coupon = m({
      key: 'coupon',
      kind: 'bond_coupon',
      stage: 'soon',
      tone: 'info',
      dueDay: 10 * DAY,
    });
    const { soon } = groupMilestones([payoff, coupon]);
    expect(soon.map((x) => x.key)).toEqual(['coupon', 'payoff']);
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

describe('referenceDay', () => {
  it('measures to the deadline while acting is possible, else to the event or the last update', () => {
    expect(referenceDay(m({ dueDay: 100 * DAY, actionDay: 58 * DAY }))).toBe(58 * DAY);
    expect(referenceDay(m({ dueDay: 100 * DAY, actionDay: 58 * DAY, tone: 'info' }))).toBe(
      100 * DAY
    );
    expect(referenceDay(m({ kind: 'backup_stale', sinceDay: 7 * DAY }))).toBe(7 * DAY);
    expect(referenceDay(m({ kind: 'backup_stale' }))).toBeNull();
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
