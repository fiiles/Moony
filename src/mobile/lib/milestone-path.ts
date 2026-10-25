import type { Milestone } from '@shared/schema';
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

/**
 * The phone page an agenda row opens (spec §8.3 item 4, "tap: the source page"): the phone's
 * counterpart of the desktop's `milestoneHref` (`src/utils/milestones.ts`), kind by kind. A kind
 * about one record (a policy, a loan, an account, a property, a watched ticker, a position) opens
 * that record's screen and needs its `sourceId`: without one it is `null`, and the row stays inert.
 * Bonds open the Dluhopisy list (the desktop opens its list too), stale balances the Finance tab,
 * the backup Nastavení → Záloha a přenos. A kind the phone has no page for would be `null` as well.
 */
export function milestonePath(m: Milestone): string | null {
  const id = m.sourceId || null;
  switch (m.kind) {
    case 'insurance_anniversary':
    case 'insurance_end':
    case 'insurance_payment':
      return id && policyPath(id);
    case 'loan_fixation_end':
    case 'loan_fixation_expired':
    case 'loan_payoff':
    case 'loan_balance_check':
      return id && loanPath(id);
    case 'bond_maturity':
    case 'bond_coupon':
      return routes.bondsList;
    case 'account_termination':
    case 'savings_rate_end':
      return id && accountPath(id);
    case 'balances_stale':
      return routes.finance;
    case 'valuation_stale':
      return id && propertyPath(id);
    case 'backup_stale':
      return settingsPath('backup');
    case 'watch_target':
      return id && watchPath(id);
    case 'stock_split':
      return id && positionPath(id);
    default:
      return null;
  }
}
