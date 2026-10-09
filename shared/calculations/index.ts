/**
 * Shared calculations module.
 * Re-exports all calculation functions and types.
 */

// Yield calculations
export { calculateAnnualYield, type YieldType, type YieldInput } from './yield-metrics';

// Realized gains calculations
export {
  calculateRealizedGains,
  type DatedConvertFn,
  type RealizedGainTransaction,
} from './realized-gains';

// Payment frequency (insurance, cashflow, forms)
export {
  PAYMENT_FREQUENCIES,
  parsePaymentFrequency,
  paymentsPerYear,
  annualizeAmount,
  type PaymentFrequency,
} from './payment-frequency';

// Loan amortization (outstanding balance, repayment schedule)
export {
  MAX_SCHEDULE_ROWS,
  amortizationSchedule,
  amortizationStep,
  loanAnchor,
  loanTermsFromWire,
  loanValuationMode,
  outstandingBalanceAt,
  paymentBelowInterest,
  todayUtcDay,
  type AmortizationSchedule,
  type AmortizationScheduleRow,
  type LoanTerms,
  type LoanValuationMode,
  type LoanWireTerms,
} from './loan-amortization';
