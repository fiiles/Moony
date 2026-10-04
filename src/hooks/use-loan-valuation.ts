import { useTranslation } from 'react-i18next';
import {
  loanAnchor,
  loanTermsFromWire,
  loanValuationMode,
  paymentBelowInterest,
  type LoanValuationMode,
} from '@shared/calculations/loan-amortization';
import type { Loan } from '@shared/schema';
import { useFormat } from '@/lib/use-format';

/**
 * How a loan's balance is valued, in words: "Amortized from <date> (<amount>) at <rate> with
 * <payment> per month". Shared by the loans table tooltip and the detail
 * page so both quote the same rule.
 */
export function useLoanValuationNote(loan: Loan): {
  text: string;
  mode: LoanValuationMode;
  warn: boolean;
} {
  const { t } = useTranslation('loans');
  const fmt = useFormat();
  const terms = loanTermsFromWire(loan);
  const anchor = loanAnchor(terms);
  const mode = loanValuationMode(terms);
  const warn = paymentBelowInterest(terms);
  const base = t(`valuation.${mode}`, {
    date: fmt.day(anchor.day),
    amount: fmt.money(anchor.amount, loan.currency),
    rate: fmt.percent(terms.annualRatePct / 100, 2),
    payment: fmt.money(terms.monthlyPayment, loan.currency),
  });
  return { text: warn ? `${base} ${t('valuation.belowInterest')}` : base, mode, warn };
}
