/**
 * Shared Schema Types for Tauri Frontend
 *
 * These are TypeScript types matching the Rust backend models,
 * without the drizzle-orm dependencies.
 */

import { z } from 'zod';
import { PAYMENT_FREQUENCIES } from './calculations/payment-frequency';

// User Profile
export interface UserProfile {
  id: number;
  name: string;
  surname: string;
  email: string;
  menuPreferences: MenuPreferences;
  currency: string;
  language?: string;
  excludePersonalRealEstate: boolean;
  coingeckoModalDismissed?: boolean;
  mcpServerEnabled?: boolean;
  createdAt: number;
}

export interface MenuPreferences {
  loans: boolean;
  insurance: boolean;
  investments: boolean;
  bonds: boolean;
  realEstate: boolean;
  /** Serialized by Rust with `default = true`; optional on input. */
  crypto?: boolean;
  otherAssets?: boolean;
}

// Bank account interest-rate zones (tiers)
export interface SavingsAccountZone {
  id: string;
  savingsAccountId: string;
  fromAmount: string;
  toAmount: string | null;
  interestRate: string;
  createdAt: number;
}

// Stock Investments
export interface StockInvestment {
  id: string;
  ticker: string;
  companyName: string;
  quantity: string;
  /**
   * Derived on read: weighted average cost per unit in the position's
   * primary (first-transaction) currency, computed from the transaction
   * record. Never stored. Converting it at today's rate is wrong for any
   * other currency — derive display-currency cost from transactions at
   * their day's rates instead (shared/calculations/cost-basis.ts).
   */
  averagePrice: string;
  /** Currency of the average price (determined by first transaction) */
  averagePriceCurrency: string;
}

export interface InvestmentTransaction {
  id: string;
  investmentId: string;
  type: string;
  ticker: string;
  companyName: string;
  quantity: string;
  pricePerUnit: string;
  currency: string;
  transactionDate: number;
  createdAt: number;
}

// Crypto
export interface CryptoInvestment {
  id: string;
  ticker: string;
  coingeckoId: string;
  name: string;
  quantity: string;
  /** Derived on read from transactions (see StockInvestment.averagePrice). */
  averagePrice: string;
}

export interface CryptoTransaction {
  id: string;
  investmentId: string;
  type: string;
  ticker: string;
  name: string;
  quantity: string;
  pricePerUnit: string;
  currency: string;
  transactionDate: number;
  createdAt: number;
}

// Bonds
export interface Bond {
  id: string;
  name: string;
  isin: string | null;
  couponValue: string;
  quantity: string;
  currency: string;
  interestRate: string;
  maturityDate: number | null;
  createdAt: number;
  updatedAt: number;
}

// Loans
export interface Loan {
  id: string;
  name: string;
  principal: string;
  currency: string;
  interestRate: string;
  interestRateValidityDate: number | null;
  monthlyPayment: string;
  startDate: number;
  endDate: number | null;
  /** Real balance entered by the user; amortization continues from it (set together with the date). */
  balanceAnchorAmount: string | null;
  /** UTC day (unix seconds) the manual balance applies to. */
  balanceAnchorDate: number | null;
  /** Read-only: amortized balance as of today in the loan's own currency (2 decimals). */
  outstandingBalance: string;
  createdAt: number;
  updatedAt: number;
}

/** One payment of a loan's repayment schedule (money as TEXT, 2 decimals). */
export interface LoanScheduleRow {
  /** UTC day the payment falls due. */
  dueDay: number;
  payment: string;
  interest: string;
  /** Negative when the payment does not cover the interest. */
  principalPart: string;
  balanceAfter: string;
}

/** Repayment schedule and totals of one loan as of today (loan detail page). */
export interface LoanSchedule {
  loanId: string;
  /** Valuation day (today, UTC). */
  asOfDay: number;
  /** Amount and day the amortization starts from. */
  anchorAmount: string;
  anchorDay: number;
  /** True for the user's manual balance, false for principal at the start date. */
  anchorIsManual: boolean;
  outstandingBalance: string;
  /** Anchor amount minus the balance as of `asOfDay` (negative when it grew). */
  principalRepaid: string;
  /** Interest included in the payments between the anchor and `asOfDay`. */
  interestPaid: string;
  paymentsMade: number;
  /** Payments left until the balance is 0; null when the schedule does not reach 0. */
  paymentsRemaining: number | null;
  payoffDay: number | null;
  rows: LoanScheduleRow[];
  /** `noPayment`, `paymentBelowInterest`, `balanceAtEndDate`, `scheduleTruncated`. */
  warnings: string[];
}

/** Extra payment, rate change or balance check that re-anchored a loan's amortization (migration 014). */
export interface LoanEvent {
  id: string;
  loanId: string;
  /** `extra_payment` | `rate_change` | `balance_check` */
  kind: LoanEventKind;
  /** UTC-midnight day of the event */
  eventDay: number;
  /** Extra payment amount, or the balance confirmed by a statement */
  amount: string | null;
  /** New annual rate in percent (`rate_change`) */
  rate: string | null;
  /** New regular payment (`rate_change`, or an `extra_payment` the bank used to lower it) */
  monthlyPayment: string | null;
  note: string | null;
  createdAt: number;
}

export type LoanEventKind = 'extra_payment' | 'rate_change' | 'balance_check';

export interface InsertLoanEvent {
  loanId: string;
  kind: LoanEventKind;
  eventDay: number;
  amount?: string | null;
  rate?: string | null;
  monthlyPayment?: string | null;
  note?: string | null;
}

// Real Estate
export interface RecurringCost {
  name: string;
  amount: number;
  frequency: string;
  currency?: string;
}

export interface RealEstate {
  id: string;
  name: string;
  address: string;
  type: string;
  purchasePrice: string;
  purchasePriceCurrency: string;
  /** UTC-midnight day the property was bought; null while unknown (ADR 0008) */
  purchaseDate: number | null;
  marketPrice: string;
  marketPriceCurrency: string;
  monthlyRent: string | null;
  monthlyRentCurrency: string | null;
  recurringCosts: RecurringCost[];
  photos: string[];
  notes: string | null;
  createdAt: number;
  updatedAt: number;
  // Helper fields from backend logic (optional in DB but present in API usually if enriched, otherwise optional)
  // Actually looking at the usage in RealEstate.tsx, these seem to be expected always or defaulted.
  // The previous code had strict types for some but not others.
  // Let's add them as optional or check if they are actually in the DB model.
  // If they are not in the DB model but computed/returned, we might need a separate type or add them here.
}

export interface RealEstateOneTimeCost {
  id: string;
  realEstateId: string;
  name: string;
  description: string | null;
  amount: string;
  currency: string;
  date: number;
  createdAt: number;
}

/** One dated estimate of a manually priced asset (real estate: whole property; other asset: per unit). */
export interface AssetValuation {
  id: string;
  /** `real_estate.id` or `other_assets.id` */
  assetId: string;
  value: string;
  currency: string;
  /** UTC-midnight day the estimate applies to */
  valuedAt: number;
  note: string | null;
  createdAt: number;
}

export interface InsertAssetValuation {
  assetId: string;
  value: string;
  currency?: string | null;
  valuedAt: number;
  note?: string | null;
}

// Real Estate Photos
export interface RealEstatePhotoBatch {
  id: string;
  realEstateId: string;
  photoDate: number;
  description: string | null;
  photos: RealEstatePhoto[];
  createdAt: number;
}

export interface RealEstatePhoto {
  id: string;
  batchId: string;
  filePath: string;
  thumbnailPath: string;
  createdAt: number;
}

// Real Estate Document
export interface RealEstateDocument {
  id: string;
  realEstateId: string;
  name: string;
  description: string | null;
  filePath: string;
  fileType: string; // 'deed' | 'contract' | 'appraisal' | 'other'
  fileSize: number | null;
  uploadedAt: number;
}

// Insurance
export interface InsuranceLimit {
  title: string;
  amount: number;
  currency: string;
}

export interface InsurancePolicy {
  id: string;
  type: string;
  provider: string;
  policyName: string;
  policyNumber: string | null;
  startDate: number;
  endDate: number | null;
  paymentFrequency: string;
  oneTimePayment: string | null;
  oneTimePaymentCurrency: string | null;
  regularPayment: string;
  regularPaymentCurrency: string;
  limits: InsuranceLimit[];
  notes: string | null;
  status: string;
  createdAt: number;
  updatedAt: number;
}

// Insurance Document
export interface InsuranceDocument {
  id: string;
  insuranceId: string;
  name: string;
  description: string | null;
  filePath: string;
  fileType: string; // 'contract' | 'certificate' | 'claim' | 'other'
  fileSize: number | null;
  uploadedAt: number;
}

// Other Assets
export interface OtherAsset {
  id: string;
  name: string;
  quantity: string;
  marketPrice: string;
  currency: string;
  averagePurchasePrice: string;
  yieldType: string;
  yieldValue: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface OtherAssetTransaction {
  id: string;
  assetId: string;
  type: string;
  quantity: string;
  pricePerUnit: string;
  currency: string;
  transactionDate: number;
  createdAt: number;
}

// Portfolio
export interface PortfolioMetrics {
  totalSavings: number;
  totalInvestments: number;
  totalCrypto: number;
  totalBonds: number;
  totalRealEstatePersonal: number;
  totalRealEstateInvestment: number;
  totalRealEstate: number;
  totalOtherAssets: number;
  totalLiabilities: number;
  totalAssets: number;
  netWorth: number;
  // Native currency breakdowns
  savingsByCurrency: Record<string, number>;
  investmentsByCurrency: Record<string, number>;
  cryptoByCurrency: Record<string, number>;
  bondsByCurrency: Record<string, number>;
  realEstateByCurrency: Record<string, number>;
  loansByCurrency: Record<string, number>;
  otherAssetsByCurrency: Record<string, number>;
}

/** Freshness of prices and exchange rates (stale-prices indicator). */
export interface PriceStatus {
  stocksStale: boolean;
  cryptoStale: boolean;
  exchangeRatesStale: boolean;
  oldestStockPriceAgeHours: number | null;
  oldestCryptoPriceAgeHours: number | null;
  exchangeRatesAgeHours: number | null;
  stocksMissingPrice: number;
  cryptoMissingPrice: number;
  /** Currency codes with no known exchange rate this session (converted 1:1 to CZK) */
  missingCurrencies: string[];
}

export interface PortfolioMetricsHistory {
  id: string;
  totalSavings: string;
  totalLoansPrincipal: string;
  totalInvestments: string;
  totalCrypto: string;
  totalBonds: string;
  totalRealEstatePersonal: string;
  totalRealEstateInvestment: string;
  totalOtherAssets: string;
  recordedAt: number;
  // Native currency breakdowns (JSON strings, '{}' when not yet populated)
  investmentsByCurrency: string;
  cryptoByCurrency: string;
  savingsByCurrency: string;
  bondsByCurrency: string;
  realEstateByCurrency: string;
  loansByCurrency: string;
  otherAssetsByCurrency: string;
  /**
   * `"live"` (recorded by the running app) or `"backfill"` (reconstructed; the
   * static classes of such a day are carried from the nearest live row)
   */
  source: string;
}

// Zod Schemas for form validation

/**
 * Minimum password length (characters) for setup, recovery and change password.
 * Keep in sync with `MIN_PASSWORD_LENGTH` in `src-tauri/src/services/auth.rs` — the
 * backend enforces the same rule.
 */
export const MIN_PASSWORD_LENGTH = 8;

export const setupSchema = z
  .object({
    name: z.string().min(1, 'validation.nameRequired'),
    surname: z.string().min(1, 'validation.surnameRequired'),
    email: z.string().email('validation.invalidEmail').optional().or(z.literal('')),
    password: z.string().min(MIN_PASSWORD_LENGTH, 'validation.passwordMinLength'),
    confirmPassword: z.string().min(1, 'validation.passwordRequired'),
  })
  .refine((data) => data.password === data.confirmPassword, {
    message: 'validation.passwordMismatch',
    path: ['confirmPassword'],
  });

export const unlockSchema = z.object({
  password: z.string().min(1, 'validation.passwordRequired'),
});

export const recoverSchema = z
  .object({
    // Trimmed so a key pasted with surrounding whitespace/newlines is accepted;
    // case, inner spaces and dashes are normalized by the backend.
    recoveryKey: z.string().trim().min(1, 'validation.recoveryKeyRequired'),
    newPassword: z.string().min(MIN_PASSWORD_LENGTH, 'validation.passwordMinLength'),
    confirmPassword: z.string().min(1, 'validation.passwordRequired'),
  })
  .refine((data) => data.newPassword === data.confirmPassword, {
    message: 'validation.passwordMismatch',
    path: ['confirmPassword'],
  });

export const changePasswordSchema = z
  .object({
    currentPassword: z.string().min(1, 'validation.currentPasswordRequired'),
    newPassword: z.string().min(MIN_PASSWORD_LENGTH, 'validation.passwordMinLength'),
    confirmPassword: z.string().min(1, 'validation.passwordRequired'),
  })
  .refine((data) => data.newPassword === data.confirmPassword, {
    message: 'validation.passwordMismatch',
    path: ['confirmPassword'],
  });

export const insertBondSchema = z.object({
  name: z.string().min(1),
  isin: z.string().nullish(),
  couponValue: z.string(),
  quantity: z.string().optional(),
  interestRate: z.string().optional(),
  maturityDate: z.date().or(z.number()).optional(),
});
export type InsertBond = z.infer<typeof insertBondSchema>;

export const insertLoanSchema = z.object({
  name: z.string().min(1, 'validation.loanNameRequired'),
  principal: z.string().min(1, 'validation.principalPositive'),
  currency: z.string().optional(),
  interestRate: z.string().optional(),
  interestRateValidityDate: z.date().or(z.number()).optional(),
  monthlyPayment: z.string().optional(),
  startDate: z.date().or(z.number()).optional(),
  endDate: z.date().or(z.number()).optional(),
  // Manual outstanding balance as of a day; both or neither (null = automatic)
  balanceAnchorAmount: z.string().nullish(),
  balanceAnchorDate: z.date().or(z.number()).nullish(),
});

// Type aliases for Insert types
export type InsertLoan = z.infer<typeof insertLoanSchema>;

// Insurance schemas
export const insertInsurancePolicySchema = z.object({
  type: z.string().min(1),
  provider: z.string().min(1),
  policyName: z.string().min(1),
  policyNumber: z.string().optional(),
  startDate: z.number(),
  endDate: z.number().optional(),
  paymentFrequency: z.enum(PAYMENT_FREQUENCIES, { error: 'validation.paymentFrequencyInvalid' }),
  oneTimePayment: z.string().optional(),
  oneTimePaymentCurrency: z.string().optional(),
  regularPayment: z.string().optional(),
  regularPaymentCurrency: z.string().optional(),
  limits: z
    .array(
      z.object({
        title: z.string(),
        amount: z.number(),
        currency: z.string(),
      })
    )
    .optional(),
  notes: z.string().optional(),
  status: z.string().optional(),
});
export type InsertInsurancePolicy = z.infer<typeof insertInsurancePolicySchema>;

// Real Estate schemas
export const insertRealEstateSchema = z.object({
  name: z.string().min(1),
  address: z.string().min(1),
  type: z.string().min(1),
  purchasePrice: z.string().optional(),
  purchasePriceCurrency: z.string().optional(),
  /** Day of the purchase as UTC-midnight unix seconds; never in the future */
  purchaseDate: z.number().optional().nullable(),
  marketPrice: z.string().optional(),
  marketPriceCurrency: z.string().optional(),
  monthlyRent: z.string().optional().nullable(),
  monthlyRentCurrency: z.string().optional(),
  recurringCosts: z
    .array(
      z.object({
        name: z.string(),
        amount: z.number(),
        frequency: z.string(),
        currency: z.string().optional(),
      })
    )
    .optional(),
  photos: z.array(z.string()).optional(),
  notes: z.string().optional(),
});
export type InsertRealEstate = z.infer<typeof insertRealEstateSchema>;

export const insertRealEstateOneTimeCostSchema = z.object({
  realEstateId: z.string(),
  name: z.string().min(1),
  description: z.string().optional(),
  amount: z.string(),
  currency: z.string().optional(),
  date: z.date().or(z.number()),
});
export type InsertRealEstateOneTimeCost = z.infer<typeof insertRealEstateOneTimeCostSchema>;

// Other Assets schemas
export const insertOtherAssetSchema = z.object({
  name: z.string().min(1),
  quantity: z.string().optional(),
  marketPrice: z.string().optional(),
  currency: z.string().optional(),
  averagePurchasePrice: z.string().optional(),
  yieldType: z.string().optional(),
  yieldValue: z.string().optional(),
});
export type InsertOtherAsset = z.infer<typeof insertOtherAssetSchema>;

export const insertOtherAssetTransactionSchema = z.object({
  assetId: z.string().optional(),
  type: z.string(),
  quantity: z.string(),
  pricePerUnit: z.string(),
  currency: z.string(),
  transactionDate: z.number(),
});
export type InsertOtherAssetTransaction = z.infer<typeof insertOtherAssetTransactionSchema>;

// Investment transaction schema
export const insertInvestmentTransactionSchema = z.object({
  investmentId: z.string().optional(),
  type: z.string(),
  ticker: z.string(),
  companyName: z.string(),
  quantity: z.string(),
  pricePerUnit: z.string(),
  currency: z.string(),
  transactionDate: z.number(),
});
export type InsertInvestmentTransaction = z.infer<typeof insertInvestmentTransactionSchema>;

// Stock Monitor schema
export const insertWatchedStockSchema = z.object({
  ticker: z.string().min(1, 'validation.tickerInvalid'),
});
export type InsertWatchedStock = z.infer<typeof insertWatchedStockSchema>;

// Stock Investment with Price (enriched type from API)
export interface StockInvestmentWithPrice extends StockInvestment {
  currentPrice: string;
  fetchedAt: number | null;
  isManualPrice: boolean;
  dividendYield: number;
  dividendCurrency: string;
  isManualDividend: boolean;
}

// Crypto Investment with Price (enriched type from API)
export interface CryptoInvestmentWithPrice extends CryptoInvestment {
  currentPrice: string;
  /** Currency of the average price (native currency from first transaction) */
  averagePriceCurrency?: string;
  fetchedAt: number | null;
}

// Cashflow Types
export interface CashflowItem {
  id: string;
  name: string;
  amount: string;
  currency: string;
  frequency: 'monthly' | 'yearly';
  itemType: 'income' | 'expense';
  category: string;
  createdAt: number;
  updatedAt: number;
}

export interface CashflowReportItem {
  id: string;
  name: string;
  amount: number;
  originalAmount: number;
  originalCurrency: string;
  originalFrequency: 'monthly' | 'yearly';
  isUserDefined: boolean;
}

export interface CashflowCategory {
  key: string;
  name: string;
  total: number;
  items: CashflowReportItem[];
  isUserEditable: boolean;
}

export interface CashflowSection {
  income: CashflowCategory[];
  expenses: CashflowCategory[];
  totalIncome: number;
  totalExpenses: number;
  netCashflow: number;
}

export interface CashflowReport {
  viewType: 'monthly' | 'yearly';
  personal: CashflowSection;
  investments: CashflowSection;
  totalIncome: number;
  totalExpenses: number;
  netCashflow: number;
}

// Actual cashflow from bank transactions (design system README §11); CZK TEXT amounts
export type CashflowRange = '6m' | '12m' | 'ytd';

export interface CashflowMonth {
  /** Unix seconds of the first day of the month, UTC midnight */
  month: number;
  income: string;
  expenses: string;
}

export interface CashflowGroup {
  /** Category id (`uncategorized` without one) or a normalized counterparty key */
  key: string;
  /** Category name as stored, or the counterparty as seen; empty for an unknown source */
  name: string;
  amount: string;
  previousAmount: string;
  count: number;
}

export interface CashflowActuals {
  range: CashflowRange;
  rangeStart: number;
  rangeEnd: number;
  previousStart: number;
  previousEnd: number;
  /** Every month of the range, oldest first, zero-filled */
  months: CashflowMonth[];
  /** Net expenses by category, largest first */
  expenseCategories: CashflowGroup[];
  /** Income by counterparty, largest first */
  incomeSources: CashflowGroup[];
  totalIncome: string;
  totalExpenses: string;
  previousIncome: string;
  previousExpenses: string;
  accountCount: number;
  transactionCount: number;
}

// Projection Types
export interface ProjectionSettings {
  id: string;
  assetType:
    'savings' | 'investments' | 'crypto' | 'bonds' | 'real_estate' | 'other_assets' | 'loans';
  /** Percent as text. '0' is an explicit 0 %; '' = not set (savings/bonds use the calculated weighted rate). */
  yearlyGrowthRate: string;
  /** CZK (base currency) as text. */
  monthlyContribution: string;
  contributionCurrency: string;
  enabled: boolean;
  createdAt?: number;
  updatedAt?: number;
}

export interface ProjectionTimelinePoint {
  date: number;
  totalAssets: number;
  totalLiabilities: number;
  netWorth: number;
  savings: number;
  investments: number;
  crypto: number;
  bonds: number;
  realEstate: number;
  otherAssets: number;
  loans: number;
}

export interface PortfolioProjection {
  horizonYears: number;
  viewType: 'monthly' | 'yearly';
  timeline: ProjectionTimelinePoint[];
  projectedNetWorth: number;
  totalContributions: number;
  totalGrowth: number;
  calculatedDefaults: CalculatedDefaults;
}

export interface CalculatedDefaults {
  savingsRate: number;
  bondsRate: number;
}

// Bank Accounts
export type AccountType = 'checking' | 'savings' | 'credit_card' | 'investment';
export type DataSource = 'manual' | 'csv_import' | 'api_sync';
export type TransactionType = 'credit' | 'debit';
export type TransactionStatus = 'booked' | 'pending';

export interface Institution {
  id: string;
  name: string;
  bic: string | null;
  country: string | null;
  logoUrl: string | null;
  createdAt: number;
}

export interface BankAccount {
  id: string;
  name: string;
  accountType: AccountType;
  iban: string | null;
  bban: string | null;
  currency: string;
  balance: string;
  institutionId: string | null;
  externalAccountId: string | null;
  dataSource: DataSource;
  lastSyncedAt: number | null;
  interestRate: string | null;
  hasZoneDesignation: boolean;
  terminationDate: number | null;
  /** UTC day the promotional interest rate ends; null when unknown or not promotional. */
  interestRateValidUntil: number | null;
  /** Exclude from portfolio balance (for operational/checking accounts) */
  excludeFromBalance: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface BankAccountWithInstitution extends BankAccount {
  institution: Institution | null;
  effectiveInterestRate: number | null;
  projectedEarnings: number | null;
}

export interface InsertBankAccount {
  name: string;
  accountType?: AccountType;
  iban?: string | null;
  bban?: string | null;
  currency?: string;
  balance?: string;
  institutionId?: string | null;
  interestRate?: string | null;
  hasZoneDesignation?: boolean;
  terminationDate?: number | null;
  /** Overwritten on update like `terminationDate`: send the stored value back, `null` clears it. */
  interestRateValidUntil?: number | null;
  /** Exclude from portfolio balance (for operational/checking accounts) */
  excludeFromBalance?: boolean;
}

export interface BankTransaction {
  id: string;
  bankAccountId: string;
  transactionId: string | null;
  type: TransactionType;
  amount: string;
  currency: string;
  description: string | null;
  counterpartyName: string | null;
  counterpartyIban: string | null;
  bookingDate: number;
  valueDate: number | null;
  categoryId: string | null;
  merchantCategoryCode: string | null;
  remittanceInfo: string | null;
  /** Category suggested by an MCP client, pending user confirmation */
  suggestedCategoryId: string | null;
  status: TransactionStatus;
  dataSource: DataSource;
  createdAt: number;
}

export interface InsertBankTransaction {
  bankAccountId: string;
  transactionId?: string | null;
  type: TransactionType;
  amount: string;
  currency?: string;
  description?: string | null;
  counterpartyName?: string | null;
  counterpartyIban?: string | null;
  bookingDate: number;
  valueDate?: number | null;
  categoryId?: string | null;
  status?: TransactionStatus;
}

export interface TransactionCategory {
  id: string;
  name: string;
  icon: string | null;
  color: string | null;
  parentId: string | null;
  sortOrder: number;
  isSystem: boolean;
  createdAt: number;
}

export interface InsertTransactionCategory {
  name: string;
  icon?: string | null;
  color?: string | null;
  parentId?: string | null;
  sortOrder?: number;
}

/** Edit a category's definition; an absent field is left unchanged. */
export interface UpdateTransactionCategory {
  name?: string;
  icon?: string;
  color?: string;
}

/** How many rows reference a category (what a delete would have to move). */
export interface CategoryUsage {
  categoryId: string;
  transactions: number;
  rules: number;
  learnedPayees: number;
  budgetGoals: number;
}

export interface TransactionFilters {
  dateFrom?: number;
  dateTo?: number;
  categoryId?: string;
  txType?: TransactionType;
  search?: string;
  limit?: number;
  offset?: number;
}

export interface TransactionQueryResult {
  transactions: BankTransaction[];
  total: number;
}

// ============================================================================
// Categorization Engine Types
// ============================================================================

export type CategorizationSource =
  | { type: 'Rule'; data: { ruleId: string; ruleName: string } }
  | { type: 'ExactMatch'; data: { payee: string } }
  | { type: 'Manual' };

export type CategorizationResult =
  { type: 'Match'; data: { categoryId: string; source: CategorizationSource } } | { type: 'None' };

/** Transaction input for the categorization engine */
export interface TransactionInput {
  id: string;
  description?: string;
  counterparty?: string;
  counterpartyIban?: string;
  amount: number;
  isCredit: boolean;
  /** Source bank account ID (for learning context) */
  bankAccountId?: string;
}

export type RuleType = 'Regex' | 'Contains' | 'Word' | 'StartsWith' | 'EndsWith';

export interface CategorizationRule {
  id: string;
  name: string;
  ruleType: RuleType;
  pattern: string;
  categoryId: string;
  priority: number;
  isActive: boolean;
  stopProcessing: boolean;
}

export interface CategorizationStats {
  activeRules: number;
  learnedPayees: number;
}

/** Learned payee entry for the management UI */
export interface LearnedPayeeEntry {
  id: string;
  /** Rule type: "payee_default", "iban_default", "iban_only_default" */
  ruleType: string;
  normalizedPayee?: string;
  originalPayee?: string;
  counterpartyIban?: string;
  categoryId: string;
  createdAt: number;
  updatedAt: number;
}

/** Custom categorization rule from the database */
export interface CustomRule {
  id: string;
  name: string;
  ruleType: string;
  pattern: string;
  categoryId: string;
  priority: number;
  isActive: boolean;
  stopProcessing: boolean;
  isSystem: boolean;
  createdAt: number;
  ibanPattern?: string;
}

/** Input for creating/updating custom rules */
export interface CustomRuleInput {
  name: string;
  ruleType: string;
  pattern: string;
  categoryId: string;
  priority: number;
  isActive: boolean;
  stopProcessing: boolean;
  ibanPattern?: string;
}

/** One shipped locale rule pack, with its enabled flag */
export interface RulePackInfo {
  packId: string;
  country: string;
  version: number;
  ruleCount: number;
  enabled: boolean;
}

/** How often one rule matched in the last N days (`ruleId` = custom id, pack rule id or learned payee id). */
export interface RuleHitCount {
  ruleId: string;
  hits: number;
  /** Booking day of the newest matching transaction. */
  lastHit: number | null;
}

/** One transaction a draft rule matches. */
export interface RuleMatchSample {
  bookingDate: number;
  description: string | null;
  counterparty: string | null;
  /** Signed decimal string (debits negative). */
  amount: string;
  currency: string;
  categoryId: string | null;
}

/** What a draft rule would match today. */
export interface RuleMatchPreview {
  matches: number;
  /** Matching transactions that have no category today. */
  uncategorized: number;
  /** The newest matches, at most five. */
  samples: RuleMatchSample[];
}

export interface CategorizationSourceCount {
  /** `rule` | `exact_match` | `own_account` | `mcp` | `manual` */
  source: string;
  count: number;
}

/** Automation rate of a period. */
export interface CategorizationOverview {
  total: number;
  categorized: number;
  bySource: CategorizationSourceCount[];
}

/** One pack rule, with its disabled flag */
export interface PackRuleInfo {
  id: string;
  name: string;
  ruleType: string;
  pattern: string;
  categoryId: string;
  priority: number;
  disabled: boolean;
}

// ============================================================================
// CSV Import Types
// ============================================================================

/** Keep only rows whose `column` equals `equals` (case/whitespace-insensitive). */
export interface CsvRowFilter {
  column: string;
  equals: string;
}

/**
 * One bank's CSV format — JSON data under `src-tauri/resources/csv-presets/`.
 * Exactly one of `amountColumn` or the `debitColumn` + `creditColumn` pair is set.
 */
export interface CsvPreset {
  schemaVersion: number;
  /** Stable kebab-case id (`revolut`). */
  id: string;
  bankName: string;
  /** ISO 3166-1 alpha-2 country of the bank. */
  country: string;
  /** Row id in the `institutions` table (`inst_revolut`) when the bank is seeded there. */
  institutionId: string | null;
  /** Header cells that must all be present for auto-detection. */
  matchHeaders: string[];
  delimiter: string;
  encoding: string;
  dateColumn: string;
  /** chrono format; may contain a time part (`%Y-%m-%d %H:%M:%S`). */
  dateFormat: string;
  amountColumn: string | null;
  debitColumn: string | null;
  creditColumn: string | null;
  /** Joined with " | " in this order into the description. */
  descriptionColumns: string[];
  counterpartyColumn: string | null;
  counterpartyIbanColumn: string | null;
  currencyColumn: string | null;
  balanceColumn: string | null;
  transactionIdColumn: string | null;
  rowFilter: CsvRowFilter | null;
  exportHelpUrl: string | null;
  sampleFixture: string | null;
}

/**
 * A per-row problem or note. `key` is an i18n key — `csvImport.*` lives in the
 * `bank_accounts` namespace, `validation.*` in `common` — and `detail` the raw
 * value it is about (the unparseable date, the filtered state, ...).
 *
 * csvImport.*: rowCannotParse, dateUnparseable, amountUnparseable, zeroAmount,
 * filteredOut, duplicate, duplicateById.
 */
export interface CsvRowMessage {
  /** 1-based file line (the header line counts). */
  line: number;
  key: string;
  detail: string | null;
}

/** Inclusive range of booking dates, unix seconds (midnight UTC). */
export interface CsvDateRange {
  from: number;
  to: number;
}

/**
 * Every data row of the file lands in exactly one bucket:
 * `importedCount + skippedDuplicates.length + errorCount + skippedZero +
 * skippedFiltered` = data rows.
 */
export interface CsvImportResult {
  /** Rows written (includes duplicates that were imported anyway). */
  importedCount: number;
  /** Duplicate hits that were imported anyway: `duplicates.length`. */
  duplicateCount: number;
  errorCount: number;
  errors: CsvRowMessage[];
  /** Duplicates imported anyway (`importAnywayLines` or `skipDuplicates: false`). */
  duplicates: CsvRowMessage[];
  /** Duplicates that were skipped. */
  skippedDuplicates: CsvRowMessage[];
  skippedZero: number;
  /** Rows dropped by the row filter (Revolut pending rows). */
  skippedFiltered: number;
  /** Booking dates of the imported rows; null when nothing was imported. */
  dateRange: CsvDateRange | null;
  /** Imported rows the categorization rules could not categorize. */
  uncategorizedCount: number;
  /** Balance after the chronologically last statement row (signed decimal string). */
  lastBalance: string | null;
  /** Rule-pack id (lowercase ISO country) suggested by the counterparty IBANs, if not enabled. */
  suggestedRulePack: string | null;
}

export interface CsvPreviewResult {
  headers: string[];
  /** First five data rows. */
  sampleRows: string[][];
  totalRows: number;
  delimiter: string;
  /**
   * role -> [header, confidence 0..1]. Roles: date, valueDate, amount, debit, credit,
   * balance, currency, description, counterparty, counterparty_iban (legacy snake_case
   * key), transactionId. A detected preset's columns come first at 1.0.
   */
  suggestedMappings: Record<string, [string, number]>;
  /** 0-based line of the header row. */
  headerRow: number;
  detectedPresetId: string | null;
  /** chrono format of the date column, date part only. */
  dateFormat: string;
  /** Day/month order cannot be told from the samples; `dateFormat` is a guess to confirm. */
  dateFormatAmbiguous: boolean;
  /** "," or ".". */
  decimalSeparator: string;
  /** Encoding the file was decoded with (`UTF-8`, `windows-1250`, ...). */
  encoding: string;
  /** Description columns to pre-select: the preset's, else the single suggested one. */
  suggestedDescriptionColumns: string[];
  /** The detected preset's row filter, resolved to the file's own header spelling. */
  suggestedRowFilter: CsvRowFilter | null;
}

export interface CsvImportConfigInput {
  /** One character; empty = auto-detect. */
  delimiter: string;
  /** Data rows to skip after the header row. */
  skipRows?: number;
  /** 0-based line of the header row; omit to auto-detect. */
  headerRow?: number | null;
  dateColumn: string;
  /** chrono format, parsed strictly; "" or "auto" detects it. */
  dateFormat: string;
  /** Single signed amount column; optional when a debit/credit pair is given. */
  amountColumn?: string | null;
  /** Debit/credit pair (amount = credit - debit); used when `amountColumn` is not set. */
  debitColumn?: string | null;
  creditColumn?: string | null;
  /** Feeds `lastBalance` only. */
  balanceColumn?: string | null;
  /** Bank-side transaction id; feeds the duplicate check. */
  transactionIdColumn?: string | null;
  descriptionColumns?: string[] | null;
  counterpartyColumn?: string | null;
  counterpartyIbanColumn?: string | null;
  currencyColumn?: string | null;
  /** "," or "."; omit/""/"auto" detects it. */
  decimalSeparator?: string | null;
  rowFilter?: CsvRowFilter | null;
  /** Skip rows the duplicate check matches (default true). */
  skipDuplicates?: boolean;
  /** File lines of duplicate rows to import anyway. */
  importAnywayLines?: number[];
  /** Force a text encoding (`Windows-1250`); omit to detect. */
  encoding?: string | null;
  /** Categories picked in the preview, by file line; applied before the rules on import. */
  categoryOverrides?: CsvCategoryOverride[];
}

/** One row's category chosen in the preview (file line → category). */
export interface CsvCategoryOverride {
  /** 1-based file line of the row. */
  line: number;
  categoryId: string;
}

/**
 * ok: will be imported. error: cannot be imported (see `message`). duplicate: matches an
 * existing or earlier transaction — skipped unless its line is in `importAnywayLines`.
 * skipped: zero amount or dropped by the row filter.
 */
export type CsvRowStatus = 'ok' | 'error' | 'duplicate' | 'skipped';

export interface CsvPreviewRow {
  line: number;
  bookingDate: number | null;
  /** Signed decimal string (`-250.5`); null when unreadable. */
  amount: string | null;
  currency: string | null;
  description: string | null;
  counterparty: string | null;
  status: CsvRowStatus;
  message: CsvRowMessage | null;
  /** Category the import would write (override or rules); null = uncategorized. */
  categoryId: string | null;
  /** `manual` for an override, else `rule` | `exact_match` | `own_account`. */
  categorySource: string | null;
}

export interface CsvImportPreview {
  /** First 200 rows in file order. */
  rows: CsvPreviewRow[];
  totalRows: number;
  okCount: number;
  errorCount: number;
  duplicateCount: number;
  /** Zero-amount and filtered rows. */
  skippedCount: number;
  /** Booking dates of the rows an import with this config would write. */
  dateRange: CsvDateRange | null;
  /** Rows that would be written with a category (rules or overrides). */
  categorizedCount: number;
}

export interface CsvImportBatch {
  id: string;
  bankAccountId: string;
  fileName: string;
  importedCount: number;
  duplicateCount: number;
  errorCount: number;
  importedAt: number;
}

// ============================================================================
// Stock Tags Types
// ============================================================================

export interface StockTagGroup {
  id: string;
  name: string;
  description: string | null;
  createdAt: number;
}

export interface InsertStockTagGroup {
  name: string;
  description?: string | null;
}

export interface StockTag {
  id: string;
  name: string;
  color: string | null;
  groupId: string | null;
  createdAt: number;
}

export interface InsertStockTag {
  name: string;
  color?: string | null;
  groupId?: string | null;
}

export interface TwrDataPoint {
  /** Calendar date as "YYYY-MM-DD" */
  date: string;
  /** Cumulative TWR in percent (0.0 at start of range) */
  twr: number;
}

export interface TwrSeries {
  /** Tag this series belongs to. null = whole portfolio or untagged. */
  tag: StockTag | null;
  /** True when this series represents stocks with no tags assigned. */
  isUntagged: boolean;
  data: TwrDataPoint[];
}

export interface StockInvestmentWithTags {
  id: string;
  ticker: string;
  companyName: string;
  quantity: string;
  averagePrice: string;
  currentPrice: string;
  currentValue: number;
  gainLoss: number;
  gainLossPercent: number;
  dividendYield: number;
  tags: StockTag[];
}

export interface TagMetrics {
  tag: StockTag;
  totalValue: number;
  totalCost: number;
  gainLoss: number;
  gainLossPercent: number;
  estimatedYearlyDividend: number;
  portfolioPercent: number;
  holdingsCount: number;
}

// ============================================================================
// MCP Server Types
// ============================================================================

export interface McpServerStatus {
  running: boolean;
  port: number;
  url: string;
  tokenSet: boolean;
  lastError: string | null;
}

// Payload for the `mcp-data-changed` event emitted by the embedded MCP server
// whenever an MCP write tool changes data (see src-tauri/src/services/mcp/mod.rs).
export interface McpDataChangedPayload {
  domain: string;
  tickers: [string, string | null][];
  earliestDate: number | null;
}

// Payload for the `history-recalculation` event emitted by the background rebuild of a stock
// ticker's value history (src-tauri/src/services/history_recalc.rs). `running` when the
// ticker's rebuild starts, then `done` (or `failed`) when its batch finishes.
export type HistoryRecalculationStatus = 'running' | 'done' | 'failed';

export interface HistoryRecalculationEvent {
  ticker: string;
  status: HistoryRecalculationStatus;
}

// ============================================================================
// Stock Monitor (watchlist) Types — spec 2026-08-17-stock-monitor-design
// ============================================================================

/** What a watchlist target waits for: a fall to it (`below`) or a rise to it (`above`). */
export type TargetDirection = 'below' | 'above';

export interface WatchedStock {
  id: string;
  ticker: string;
  targetPrice: string | null;
  targetDirection: TargetDirection | null;
  notes: string;
  createdAt: number;
  updatedAt: number;
}

export interface WatchedStockRow {
  id: string;
  ticker: string;
  targetPrice: string | null;
  targetDirection: TargetDirection | null;
  notes: string;
  shortName: string | null;
  longName: string | null;
  currency: string | null;
  currentPrice: string | null;
  previousClose: string | null;
  fiftyTwoWeekLow: string | null;
  fiftyTwoWeekHigh: string | null;
  exchange: string | null;
  priceFetchedAt: number | null;
  heldInvestmentId: string | null;
  isHeld: boolean;
  /** When the stock was added to the watchlist (unix seconds). */
  followedAt: number;
}

export interface StockMonitorDetail {
  ticker: string;
  followed: boolean;
  targetPrice: string | null;
  targetDirection: TargetDirection | null;
  notes: string;
  shortName: string | null;
  longName: string | null;
  currency: string | null;
  currentPrice: string | null;
  previousClose: string | null;
  fiftyTwoWeekLow: string | null;
  fiftyTwoWeekHigh: string | null;
  marketCap: string | null;
  peRatio: string | null;
  trailingDividendYield: string | null; // Raw Yahoo fraction (0.0044 = 0.44%); multiply by 100 for display.
  exchange: string | null;
  priceFetchedAt: number | null;
  heldInvestmentId: string | null;
  isHeld: boolean;
  /** When the stock was added to the watchlist (unix seconds); null when not followed. */
  followedAt: number | null;
}

export interface StockPricePoint {
  timestamp: number;
  price: number;
  currency: string;
}

// Company data of a stock for the position detail (src-tauri/src/models/company_info.rs).
// Read from the stock_data cache; a field is null when Yahoo never reported it.
export interface StockCompanyInfo {
  ticker: string;
  sector: string | null;
  industry: string | null;
  peRatio: string | null;
  forwardPe: string | null;
  /** Whole number in the listing currency. */
  marketCap: string | null;
  beta: string | null;
  fiftyTwoWeekHigh: string | null;
  fiftyTwoWeekLow: string | null;
  /** Annual dividend per share in the listing currency. */
  dividendRate: string | null;
  /** Raw Yahoo fraction (0.0331 = 3.31 %); multiply by 100 for display. */
  dividendYield: string | null;
  /** Yahoo instrument class ("EQUITY", "ETF", …). */
  quoteType: string | null;
  /** Currency of the prices and figures above (the listing currency). */
  currency: string | null;
  /** When the metadata was last fetched (unix seconds); null when it never was. */
  metadataFetchedAt: number | null;
}

// Backup / restore / export (src-tauri/src/models/backup.rs)
export interface BackupFileEntry {
  path: string;
  bytes: number;
}

export interface BackupManifest {
  format: number;
  appVersion: string;
  createdAt: number;
  migrations: string[];
  files: BackupFileEntry[];
}

export interface BackupInspection {
  manifest: BackupManifest;
  archivePath: string;
  archiveBytes: number;
  attachmentCount: number;
  newerThanApp: boolean;
}

export interface IntegrityReport {
  ok: boolean;
  messages: string[];
}

export interface DataLocation {
  dataDir: string;
  dbPath: string;
  totalBytes: number;
}

export interface ExportSummary {
  path: string;
  tableCount: number;
  rowCount: number;
}

// First-run onboarding (src-tauri/src/models/onboarding.rs)
export interface OnboardingProgress {
  hasAccount: boolean;
  hasTransactions: boolean;
  hasInvestment: boolean;
  hasBudget: boolean;
  lastBackupAt: number | null;
  checklistDismissed: boolean;
  completedAt: number | null;
}

// ============================================================================
// Stock CSV import (src-tauri/src/services/stock_import/types.rs)
// Columns are 0-based indexes into the header row.
// ============================================================================

export type StockTradeDirection = 'buy' | 'sell';
export type StockTypeValueAction = 'buy' | 'sell' | 'skip';
export type StockCurrencyMode = 'column' | 'fixed' | 'instrument';
export type StockDirectionMode = 'typeColumn' | 'quantitySign';

/** Built-in sources; a hand mapping is `custom`, a saved format `format:<id>`. */
export type StockImportSourceId = 'xtb' | 'trading212' | 'degiro' | 'ibkr' | 'moony' | 'custom';

export interface StockTypeValueMapping {
  value: string;
  action: StockTypeValueAction;
}

export interface StockImportTransforms {
  xtbComment?: boolean;
  xtbSymbols?: boolean;
  assetClassColumn?: number | null;
  assetClassAllowed?: string[];
  /** Degiro: the broker's id is the order's, shared by its fills (kept by a remembered format). */
  brokerIdPerOrder?: boolean;
}

export interface StockInstrumentOverride {
  key: string;
  ticker?: string | null;
  name?: string | null;
  /**
   * The currency of trades whose file names none (instrument currency mode); a currency the file
   * states is never overridden. `GBX`: the file's prices are in pence, stored as GBP ÷ 100.
   */
  currency?: string | null;
  skip?: boolean;
}

export interface StockImportConfig {
  source: string;
  delimiter: string;
  encoding: string;
  headerRow: number;
  skipRows?: number;
  dateColumn: number;
  dateFormat: string;
  symbolColumn?: number | null;
  isinColumn?: number | null;
  nameColumn?: number | null;
  quantityColumn: number;
  priceColumn: number;
  currencyMode: StockCurrencyMode;
  currencyColumn?: number | null;
  fixedCurrency?: string | null;
  directionMode: StockDirectionMode;
  typeColumn?: number | null;
  typeValues?: StockTypeValueMapping[];
  decimalSeparator: string;
  externalIdColumn?: number | null;
  transforms?: StockImportTransforms;
  instrumentOverrides?: StockInstrumentOverride[];
  importAnywayLines?: number[];
}

export interface StockCsvInspectOptions {
  source?: string | null;
  headerRow?: number | null;
  skipRows?: number | null;
  encoding?: string | null;
  delimiter?: string | null;
}

/** Roles: date, type, symbol, isin, name, quantity, price, currency, externalId, fee. */
export interface StockColumnSuggestion {
  role: string;
  column: number;
  confidence: number;
}

export interface StockTypeValueStat {
  value: string;
  count: number;
  firstLine: number;
  suggested: StockTypeValueAction;
}

export interface StockColumnValues {
  column: number;
  values: StockTypeValueStat[];
}

export interface StockCsvInspection {
  fileName: string;
  encoding: string;
  delimiter: string;
  headerRow: number;
  headers: string[];
  sampleRows: string[][];
  rowCount: number;
  detectedSource: string | null;
  config: StockImportConfig | null;
  suggestions: StockColumnSuggestion[];
  columnValues: StockColumnValues[];
  dateFormat: string | null;
  dateFormatAmbiguous: boolean;
  decimalSeparator: string;
  hasFeeColumn: boolean;
}

export type StockRowStatus = 'new' | 'duplicate' | 'skipped' | 'error';

export interface StockRowMessage {
  line: number;
  key: string;
  detail: string | null;
}

export interface StockPreviewRow {
  line: number;
  day: number | null;
  direction: StockTradeDirection | null;
  instrumentKey: string | null;
  ticker: string | null;
  quantity: string | null;
  price: string | null;
  currency: string | null;
  status: StockRowStatus;
  message: StockRowMessage | null;
}

export type StockInstrumentStatus = 'existing' | 'new' | 'missingSymbol' | 'skipped';

export interface StockImportInstrument {
  key: string;
  symbol: string | null;
  isin: string | null;
  name: string | null;
  currency: string | null;
  tradeCount: number;
  ticker: string | null;
  status: StockInstrumentStatus;
  positionCurrency: string | null;
}

export interface StockImportCounts {
  total: number;
  willImport: number;
  duplicates: number;
  skipped: number;
  errors: number;
}

export interface StockImportPreview {
  instruments: StockImportInstrument[];
  rows: StockPreviewRow[];
  counts: StockImportCounts;
  dateRange: CsvDateRange | null;
  hasFeeColumn: boolean;
}

export interface StockInstrumentQuery {
  key: string;
  symbol: string | null;
  isin: string | null;
  name: string | null;
  currency: string | null;
}

export interface StockInstrumentCandidate {
  symbol: string;
  name: string;
  exchange: string;
  /**
   * What Yahoo reports for the quote of the chosen listing (`best`); the guess from the exchange
   * suffix for the other candidates. `GBX` stands for pence: prices are a hundredth of GBP.
   */
  currency: string;
}

export interface StockInstrumentResolution {
  key: string;
  candidates: StockInstrumentCandidate[];
  best: StockInstrumentCandidate | null;
  lookupFailed: boolean;
}

export interface StockImportResult {
  batchId: string | null;
  imported: number;
  duplicates: number;
  skipped: number;
  errors: number;
  newPositions: string[];
  updatedPositions: string[];
  messages: StockRowMessage[];
  earliestDay: number | null;
}

export interface StockImportBatch {
  id: string;
  fileName: string;
  source: string;
  tradeCount: number;
  remainingCount: number;
  createdAt: number;
}

export interface StockImportUndoResult {
  removed: number;
  removedPositions: string[];
  tickers: string[];
  earliestDay: number | null;
}

export interface SavedStockImportFormat {
  id: string;
  name: string;
  headerSignature: string;
  config: StockImportConfig;
  createdAt: number;
}
