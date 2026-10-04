/**
 * Shared calculations module.
 * Re-exports all calculation functions and types.
 */

// Holding-level calculations
export {
  calculateMarketValue,
  calculateTotalCost,
  calculateGainLoss,
  calculateGainLossPercent,
  calculateHoldingMetrics,
  type HoldingInput,
  type HoldingMetrics,
} from './holding-metrics';

// Portfolio-level calculations
export {
  sumHoldingValues,
  calculatePortfolioTotals,
  findTopPerformer,
  type PortfolioTotals,
} from './portfolio-metrics';

// Yield calculations
export {
  calculateAnnualYield,
  calculateYieldPercentOnCost,
  calculateYieldPercentOnMarket,
  type YieldType,
  type YieldInput,
} from './yield-metrics';

// Crypto-specific calculations
export {
  calculateCryptoHoldingMetrics,
  calculateCryptoPortfolioMetrics,
  findLargestHolding,
  mapCryptoInvestmentToHolding,
  type CryptoHoldingInput,
  type CryptoHoldingData,
  type CryptoPortfolioMetrics,
} from './crypto-metrics';

// Realized gains calculations
export {
  calculatePositionCostBasis,
  type CostBasisTransaction,
  type DatedConvertFn,
  type PositionCostBasis,
} from './cost-basis';

export { calculateRealizedGains, type RealizedGainTransaction } from './realized-gains';

// Change/percentage calculations
export {
  calculatePercentageChange,
  calculateAbsoluteChange,
  calculateNetWorth,
  calculateTotalAssets,
  calculateAllocationPercentage,
  type AssetComponents,
} from './change-metrics';

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
