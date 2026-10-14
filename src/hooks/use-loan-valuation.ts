import { useTranslation } from 'react-i18next';
import {
  dayFloor,
  ledgerValuationMode,
  loanAnchor,
  loanBalanceAt,
  loanLedgerFromWire,
  paymentBelowInterest,
  planFrom,
  todayUtcDay,
  type LoanValuationMode,
} from '@shared/calculations/loan-amortization';
import type { Loan } from '@shared/schema';
import { useFormat } from '@/lib/use-format';

/**
 * How a loan's balance is valued, in words: "Amortized from <date> (<amount>) at <rate> with
 * <payment> per month", or, with a manual balance, "Manually entered balance <amount> as of
 * <date> — it holds until you change it" (the newest manual record). Shared by the loans table
 * tooltip and the detail page so both quote the same rule.
 */
export function useLoanValuationNote(loan: Loan): {
  text: string;
  mode: LoanValuationMode;
  warn: boolean;
} {
  const { t } = useTranslation('loans');
  const fmt = useFormat();
  const today = todayUtcDay();
  const ledger = loanLedgerFromWire(loan);
  const mode = ledgerValuationMode(loanBalanceAt(ledger, today));
  const plan = planFrom(ledger, today);
  const warn = paymentBelowInterest(plan);
  // Manual: the newest manual record (the mirror on the loan); else the estimated segment's anchor
  const manual =
    mode === 'manual' && loan.balanceAnchorAmount !== null && loan.balanceAnchorDate !== null;
  const anchor = loanAnchor(plan);
  const base = t(`valuation.${mode}`, {
    date: fmt.day(manual ? dayFloor(loan.balanceAnchorDate!) : anchor.day),
    amount: fmt.money(
      manual ? Number(loan.balanceAnchorAmount) || 0 : anchor.amount,
      loan.currency
    ),
    rate: fmt.percent(plan.annualRatePct / 100, 2),
    payment: fmt.money(plan.monthlyPayment, loan.currency),
  });
  return { text: warn ? `${base} ${t('valuation.belowInterest')}` : base, mode, warn };
}
