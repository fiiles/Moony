import { describe, expect, it } from 'vitest';
import type { Milestone, MilestoneKind } from '@shared/schema';
import { milestoneHref } from '@/utils/milestones';
import {
  accountPath,
  loanPath,
  policyPath,
  positionPath,
  propertyPath,
  routes,
  settingsPath,
  watchPath,
} from '@/mobile/shell/routes';
import { milestonePath } from './milestone-path';

function milestone(kind: MilestoneKind, sourceId: string | null = 'src-1'): Milestone {
  return {
    key: `${kind}:test`,
    kind,
    stage: 'now',
    tone: 'action',
    sourceId,
    title: 'Test',
    dueDay: null,
    actionDay: null,
    sinceDay: null,
    amount: null,
    currency: null,
    referenceAmount: null,
    direction: null,
    count: null,
  };
}

/** Every kind the backend lists, with the phone page it opens (the desktop's `milestoneHref`). */
const EXPECTED: Record<MilestoneKind, string> = {
  insurance_anniversary: policyPath('src-1'),
  insurance_end: policyPath('src-1'),
  insurance_payment: policyPath('src-1'),
  loan_fixation_end: loanPath('src-1'),
  loan_fixation_expired: loanPath('src-1'),
  loan_payoff: loanPath('src-1'),
  loan_balance_check: loanPath('src-1'),
  bond_maturity: routes.bondsList,
  bond_coupon: routes.bondsList,
  account_termination: accountPath('src-1'),
  savings_rate_end: accountPath('src-1'),
  balances_stale: routes.finance,
  valuation_stale: propertyPath('src-1'),
  backup_stale: settingsPath('backup'),
  watch_target: watchPath('src-1'),
  stock_split: positionPath('src-1'),
};

describe('milestonePath', () => {
  it.each(Object.entries(EXPECTED) as [MilestoneKind, string][])(
    'opens the phone page of %s',
    (kind, path) => {
      expect(milestonePath(milestone(kind))).toBe(path);
    }
  );

  it('covers every kind the desktop links (no kind is left inert by mistake)', () => {
    for (const kind of Object.keys(EXPECTED) as MilestoneKind[]) {
      // The desktop has a page for each; so does the phone
      expect(milestoneHref(milestone(kind))).not.toBe('');
      expect(milestonePath(milestone(kind))).not.toBeNull();
    }
  });

  it('is null for a record kind without its source', () => {
    for (const kind of [
      'insurance_end',
      'loan_payoff',
      'account_termination',
      'savings_rate_end',
      'valuation_stale',
      'watch_target',
      'stock_split',
    ] as MilestoneKind[]) {
      expect(milestonePath(milestone(kind, null))).toBeNull();
      expect(milestonePath(milestone(kind, ''))).toBeNull();
    }
  });

  it('opens a list or a settings page without a source', () => {
    expect(milestonePath(milestone('bond_coupon', null))).toBe(routes.bondsList);
    expect(milestonePath(milestone('balances_stale', null))).toBe(routes.finance);
    expect(milestonePath(milestone('backup_stale', null))).toBe(settingsPath('backup'));
  });

  it('escapes a ticker that a path cannot hold', () => {
    expect(milestonePath(milestone('watch_target', 'BRK/B'))).toBe('/wealth/watch/BRK%2FB');
  });
});
