/**
 * Shared Schema Types for Tauri Frontend
 *
 * These are TypeScript types matching the Rust backend models,
 * without the drizzle-orm dependencies.
 */

import { z } from 'zod';
import { PAYMENT_FREQUENCIES } from './calculations/payment-frequency';

// User Profile
/** The colour theme choice: `system` follows the OS appearance. */
export type ThemePreference = 'system' | 'light' | 'dark';

export interface UserProfile {
  id: number;
  name: string;
  surname: string;
  email: string;
  menuPreferences: MenuPreferences;
  currency: string;
  language?: string;
  excludePersonalRealEstate: boolean;
  mcpServerEnabled?: boolean;
  /** ISO 3166-1 alpha-2 (`"CZ"`), null until chosen (wizard, Settings). */
  country: string | null;
  /** Days of cancellation notice the insurance reminders use (the stored value, else 50); null = no anniversary reminder. */
  insuranceNoticeDays: number | null;
  theme: ThemePreference;
  createdAt: number;
}

// Biometric unlock on the phone (Rust `models::biometric`)
/** Which biometry the phone offers, for the unlock button's icon and wording. */
export type BiometryType = 'faceId' | 'touchId' | 'biometric' | 'windowsHello' | 'none';

/** `biometric_unlock_status`: readable while the database is still locked. */
export interface BiometricStatus {
  /** Biometry-protected storage can be used right now (biometry enrolled, passcode set). */
  available: boolean;
  /** The person switched biometric unlock on for this profile on this phone. */
  enrolled: boolean;
  biometryType: BiometryType;
  /** Why biometrics are not available, as an i18n key (`mobile:biometrics.unavailable`). */
  reason: string | null;
  /** This platform has biometric unlock at all (`false` where the keychain plugin is not built in). */
  supported: boolean;
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
   * their day's rates instead (backend `services/cost_basis.rs`).
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
  /** Yahoo Finance symbol (`BTC-USD`); null until resolved. */
  yahooSymbol: string | null;
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
  /** UTC day (unix seconds) the bond was bought; null while unknown (history then counts it on every day). */
  purchaseDate: number | null;
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
  /** Read-only mirror of the newest manual balance record (`balance_check` event); null without one. */
  balanceAnchorAmount: string | null;
  /** UTC day (unix seconds) of the newest manual balance record. */
  balanceAnchorDate: number | null;
  /** Read-only: balance as of today from the loan ledger, in the loan's own currency (2 decimals). */
  outstandingBalance: string;
  /** Read-only: `manual` when a manual balance record holds today's balance, else `estimated`. */
  balanceMode: LoanBalanceMode;
  /** Read-only: the loan's events, oldest first (day, then creation time). */
  events: LoanEvent[];
  createdAt: number;
  updatedAt: number;
}

export type LoanBalanceMode = 'estimated' | 'manual';

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

/** Extra payment, rate change or manual balance record of a loan; the loan ledger replays them (migrations 014, 019). */
export interface LoanEvent {
  id: string;
  loanId: string;
  /** `extra_payment` | `rate_change` | `balance_check` (the manual balance record) */
  kind: LoanEventKind;
  /** UTC-midnight day of the event */
  eventDay: number;
  /** Extra payment amount, or the manual balance of a `balance_check` record */
  amount: string | null;
  /** New annual rate in percent (`rate_change`) */
  rate: string | null;
  /** New regular payment (`rate_change`, or an `extra_payment` the bank used to lower it) */
  monthlyPayment: string | null;
  /** Annual rate in force before a `rate_change`; null on legacy rows and other kinds */
  previousRate: string | null;
  /** Regular payment in force before an event that sets a new one; null on legacy rows and other kinds */
  previousMonthlyPayment: string | null;
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

/** A photo's bytes load through the `moony-file` scheme by id (`src/utils/attachment-url.ts`). */
export interface RealEstatePhoto {
  id: string;
  batchId: string;
  createdAt: number;
}

// Real Estate Document
export interface RealEstateDocument {
  id: string;
  realEstateId: string;
  name: string;
  description: string | null;
  /** Stored MIME type, or the one the legacy file's extension implies. */
  mimeType: string;
  /** Lowercased extension of the original file name, for the file tile. */
  extension: string | null;
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
  /** Stored MIME type, or the one the legacy file's extension implies. */
  mimeType: string;
  /** Lowercased extension of the original file name, for the file tile. */
  extension: string | null;
  fileType: string; // 'contract' | 'certificate' | 'claim' | 'other'
  fileSize: number | null;
  uploadedAt: number;
}

// Attachments (ADR 0017)
/** Which attachment table a row id belongs to. */
export type AttachmentKind = 'photo' | 'real-estate-document' | 'insurance-document';

/** The encrypted store, the conversion of plain files from earlier versions, and leftovers. */
export interface AttachmentStatus {
  /** `attachments.db` is attached and usable. */
  storeAvailable: boolean;
  /** Attachments stored encrypted. */
  stored: number;
  /** Attachments still in plain files that are waiting for the conversion. */
  legacyPending: number;
  /** Attachments whose plain file is missing on disk. */
  missing: number;
  /** Attachments the last conversion run could not convert (retried at the next unlock). */
  failed: number;
  /** A conversion run is in progress. */
  converting: boolean;
  /** Items of the current or last run converted so far. */
  done: number;
  /** Items of the current or last run. */
  total: number;
  /** Plain originals the app found and does not delete: files and old data folders. */
  leftovers: string[];
}

// Other Assets
export interface OtherAsset {
  id: string;
  name: string;
  quantity: string;
  /** Today's price per unit: the newest valuation, else the stored market price. */
  marketPrice: string;
  /** The currency of `marketPrice`: the newest valuation's, else the asset's own. */
  marketPriceCurrency: string;
  /** The asset's own currency: the purchase average's (and the fallback price's). */
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
/**
 * Current net worth in `currency`. Class totals are decimal TEXT, `null` when nothing in the
 * class could be converted; the totals sum what converted and `fxQuality` names the rest.
 * The `*ByCurrency` maps stay native (amounts in their own currencies).
 */
export interface PortfolioMetrics {
  currency: string;
  fxQuality: ConversionQuality;
  totalSavings: string | null;
  totalInvestments: string | null;
  totalCrypto: string | null;
  totalBonds: string | null;
  /** Always reported, also when personal real estate is excluded from the totals. */
  totalRealEstatePersonal: string | null;
  totalRealEstateInvestment: string | null;
  /** The real estate counted in `totalAssets` (personal left out when excluded). */
  totalRealEstate: string | null;
  totalOtherAssets: string | null;
  /** `null` when the loans hold amounts but none of them could be converted. */
  totalLiabilities: string | null;
  /** `null` when the asset classes hold amounts but none of them could be converted. */
  totalAssets: string | null;
  /** `null` unless both `totalAssets` and `totalLiabilities` are known. */
  netWorth: string | null;
  // Native currency breakdowns
  savingsByCurrency: Record<string, number>;
  investmentsByCurrency: Record<string, number>;
  cryptoByCurrency: Record<string, number>;
  bondsByCurrency: Record<string, number>;
  /** Personal properties left out when excluded, like `totalRealEstate`. */
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
  /** Currencies entering converted totals (open positions, transactions, entities, profile) without a current quote; excluded from those totals */
  missingCurrencies: string[];
  /** Held currencies whose newest stored exchange rate is more than 14 days old */
  staleCurrencies: string[];
}

/**
 * One recorded day of the portfolio history in the response currency (converted at that
 * day's rates). Amounts are decimal TEXT, `null` when nothing in the class converted.
 */
export interface PortfolioHistoryPoint {
  recordedAt: number;
  /**
   * `"live"` (recorded by the running app) or `"backfill"` (reconstructed; the
   * static classes of such a day are carried from the nearest live row)
   */
  source: string;
  isStale: boolean;
  totalSavings: string | null;
  totalLoansPrincipal: string | null;
  totalInvestments: string | null;
  totalCrypto: string | null;
  totalBonds: string | null;
  totalRealEstatePersonal: string | null;
  totalRealEstateInvestment: string | null;
  totalOtherAssets: string | null;
  /** Every converted asset class (personal real estate included) minus the loans. */
  netWorth: string | null;
}

/** The portfolio history in `currency`, oldest day first. */
export interface PortfolioHistoryResponse {
  currency: string;
  fxQuality: ConversionQuality;
  points: PortfolioHistoryPoint[];
}

/**
 * One dated fact that can explain a step of the net-worth line: a purchase, a revaluation, a
 * new loan, a manual balance, a day of trades. It carries no value of its own; amounts are
 * native, as entered.
 */
export interface DatedFact {
  /** UTC day the fact is dated. */
  day: number;
  /**
   * `buy` (a purchase, or a day of purchases), `sell`, `start` (a loan), `balance` (a balance
   * record), `extraPayment`, `valuation` or `firstValuation` (the first estimate of a property).
   */
  kind: string;
  /**
   * The history class it moves: `savings`, `investments`, `crypto`, `bonds`, `realEstate`,
   * `loans` or `otherAssets`.
   */
  class: string;
  /**
   * The account, property, loan, bond or asset; for a day of trades up to three tickers or
   * asset names, comma separated.
   */
  name: string;
  /** As entered, in `currency` (decimal TEXT); `null` for a day of trades or an amount not entered. */
  amount: string | null;
  currency: string | null;
  /** For a manual loan balance: what the ledger held the day before, in `currency`. */
  previousAmount: string | null;
  /** Distinct tickers or assets traded that day; `null` for every other fact. */
  count: number | null;
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
  // The UTC day the bond was bought (not in the future); history counts it from that day on
  purchaseDate: z.number().optional().nullable(),
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
  /** Day the market price applies to (UTC-midnight unix seconds); dates the first valuation on create */
  marketPriceDate: z.number().optional().nullable(),
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

// Manual stock split (Rust `services::stock_splits::types::InsertStockSplit`, spec D11). The
// backend validates the ratio and the day; `effectiveDay` is the UTC midnight of the ex-date.
export const insertStockSplitSchema = z.object({
  ticker: z.string().min(1, 'validation.tickerRequired'),
  effectiveDay: z.number().int(),
  numerator: z.string().regex(/^\d+$/, 'validation.stockSplitRatioInvalid'),
  denominator: z.string().regex(/^\d+$/, 'validation.stockSplitRatioInvalid'),
});
export type InsertStockSplit = z.infer<typeof insertStockSplitSchema>;

// Stock Monitor schema
export const insertWatchedStockSchema = z.object({
  ticker: z.string().min(1, 'validation.tickerInvalid'),
});
export type InsertWatchedStock = z.infer<typeof insertWatchedStockSchema>;

/**
 * Positions (ADR 0011): native facts plus figures in the response's report currency. Converted
 * amounts are decimal TEXT at the currency's minor unit and `null` when they cannot be converted
 * (the currency is then listed in the response's `fxQuality`).
 */
export interface EnrichedStockInvestment {
  id: string;
  ticker: string;
  companyName: string;
  quantity: string;
  /** Native weighted average cost, in `averagePriceCurrency`. */
  averagePrice: string;
  averagePriceCurrency: string;
  /** Current price in its own currency, "0" when unpriced (see `price`). */
  originalPrice: string;
  /** Currency of the current price. */
  currency: string;
  fetchedAt: number | null;
  isManualPrice: boolean;
  /** Yearly dividend per share in `dividendCurrency`, 0 without dividend data. */
  originalDividendYield: number;
  dividendCurrency: string | null;
  isManualDividend: boolean;
  /** Current price in its own currency; null when unpriced. */
  price: string | null;
  priceCurrency: string | null;
  value: string | null;
  costBasis: string | null;
  unrealizedGain: string | null;
  unrealizedGainPercent: number | null;
  realizedGain: string | null;
  /** Yearly dividend per share in `dividendCurrency`; null without dividend data. */
  dividendPerShare: string | null;
  /** Yearly dividend of the whole position in the report currency. */
  yearlyDividend: string | null;
}

export interface EnrichedCryptoInvestment {
  id: string;
  ticker: string;
  /** Yahoo Finance symbol (`BTC-USD`); null until resolved. */
  yahooSymbol: string | null;
  name: string;
  quantity: string;
  /** Native weighted average cost, in `averagePriceCurrency`. */
  averagePrice: string;
  averagePriceCurrency: string;
  /** Current price in its own currency, "0" when unpriced (see `price`). */
  originalPrice: string;
  /** Currency of the current price. */
  currency: string;
  fetchedAt: number | null;
  isManualPrice: boolean;
  /** Current price in its own currency; null when unpriced. */
  price: string | null;
  priceCurrency: string | null;
  value: string | null;
  costBasis: string | null;
  unrealizedGain: string | null;
  unrealizedGainPercent: number | null;
  realizedGain: string | null;
}

/**
 * Sums over every known amount: `value`, `costBasis` and `realizedGain` where known,
 * `unrealizedGain` (and its percent) of the positions whose value and cost are both known,
 * `yearlyDividend` of the positions whose value is known. A total is `null` when positions
 * hold amounts for it but none of them could be converted.
 */
export interface PositionTotals {
  value: string | null;
  costBasis: string | null;
  unrealizedGain: string | null;
  /** `unrealizedGain` in percent of the cost of the same positions; 0 without a cost. */
  unrealizedGainPercent: number;
  realizedGain: string | null;
  yearlyDividend: string | null;
}

export interface StockPositions {
  currency: string;
  fxQuality: ConversionQuality;
  items: EnrichedStockInvestment[];
  totals: PositionTotals;
}

/** One stock position; `fxQuality` covers this position only. */
export interface StockPosition {
  currency: string;
  fxQuality: ConversionQuality;
  investment: EnrichedStockInvestment;
}

export interface InvestmentWithDetails {
  currency: string;
  fxQuality: ConversionQuality;
  investment: EnrichedStockInvestment;
  transactions: InvestmentTransaction[];
  tags: StockTag[];
}

export interface CryptoPositions {
  currency: string;
  fxQuality: ConversionQuality;
  items: EnrichedCryptoInvestment[];
  totals: PositionTotals;
}

// Cashflow Types
/** How often a planned amount recurs, and the period the planning report totals over. */
export type CashflowPeriod = 'monthly' | 'quarterly' | 'yearly';

export interface InsertCashflowItem {
  name: string;
  amount: string;
  currency?: string | null;
  frequency: CashflowPeriod;
  itemType: 'income' | 'expense';
  category: string;
  replacementSourceId?: string | null;
}
export interface CashflowItem {
  replacementSourceId: string | null;
  id: string;
  name: string;
  amount: string;
  currency: string;
  frequency: CashflowPeriod;
  itemType: 'income' | 'expense';
  category: string;
  createdAt: number;
  updatedAt: number;
}

export interface CashflowOriginalComponent {
  amount: string;
  currency: string;
  frequency: string;
}

export interface CashflowReportItem {
  originalComponents: CashflowOriginalComponent[];
  /** Components with a usable target conversion, including known zero-valued components. */
  convertedComponentCount: number;
  fxQuality: FxQuality;
  sourceId: string | null;
  replacementSourceId: string | null;
  category: string;
  itemType: string;
  id: string;
  name: string;
  amount: string;
  originalAmount: string;
  originalCurrency: string;
  originalFrequency: CashflowPeriod;
  isUserDefined: boolean;
  /** False when this row or its whole category is out of the plan; `amount` is still filled. */
  included: boolean;
  /** False when the row's own key is stored, whatever its category says. */
  selfIncluded: boolean;
}

export interface CashflowCategory {
  key: string;
  name: string;
  /** Sum of the included items only. */
  total: string;
  items: CashflowReportItem[];
  isUserEditable: boolean;
  /** False when the whole category is out of the plan. */
  included: boolean;
}

/** Count a planning row in or leave it out: `category:<key>`, an automatic source id, or `item:<id>`. */
export interface SetCashflowPlanInclusion {
  key: string;
  included: boolean;
}

export interface CashflowSection {
  income: CashflowCategory[];
  expenses: CashflowCategory[];
  totalIncome: string;
  totalExpenses: string;
  netCashflow: string;
}

export interface CashflowReport {
  reportCurrency: string;
  quality: ReportQuality;
  ratesAsOf: number | null;
  automaticSources: CashflowReportItem[];
  asOf: number;
  estimate: boolean;
  viewType: CashflowPeriod;
  personal: CashflowSection;
  investments: CashflowSection;
  totalIncome: string;
  totalExpenses: string;
  netCashflow: string;
}

// Actual cashflow from bank transactions for one calendar period (design system README §11);
// report-currency TEXT amounts
export interface CashflowMonth {
  /** Unix seconds of the first day of the month, UTC midnight */
  month: number;
  income: string;
  expenses: string;
}

export interface CashflowGroup {
  /** Category id, or `uncategorized` */
  key: string;
  /** Category name as stored; empty for uncategorized */
  name: string;
  flowKind: FlowKind;
  /** Expense rows debit-positive (refunds lower them), every other role credit-positive */
  amount: string;
  /** Booked rows behind the group, including rows without a report conversion */
  count: number;
}

export interface CashflowActuals {
  reportCurrency: string;
  periodStart: number;
  periodEnd: number;
  quality: ReportQuality;
  /** Chart window, oldest first, zero-filled: 12 months ending with the period's last month */
  months: CashflowMonth[];
  totalIncome: string;
  totalExpenses: string;
  /** totalIncome − totalExpenses */
  netBalance: string;
  /** netBalance + the sum of transfers */
  bankCashMovement: string;
  averageIncome: string | null;
  averageExpenses: string | null;
  averageCompletedMonths: number[];
  /** Net expenses by category, largest first */
  expenseCategories: CashflowGroup[];
  /** Income by category, largest first; uncategorized credits form their own group */
  incomeCategories: CashflowGroup[];
  /** Money transfers (investing, saving, between accounts) by category, largest movement first */
  transfers: CashflowGroup[];
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
  /** Monthly amount as text, in `contributionCurrency` (the currency the user typed). */
  monthlyContribution: string;
  /** ISO code of `monthlyContribution`; the projection converts it into its report currency. */
  contributionCurrency: string;
  enabled: boolean;
  createdAt?: number;
  updatedAt?: number;
}

/** One projected point; amounts are decimal text in the projection's `currency`. */
export interface ProjectionTimelinePoint {
  date: number;
  totalAssets: string;
  totalLiabilities: string;
  netWorth: string;
  savings: string;
  investments: string;
  crypto: string;
  bonds: string;
  realEstate: string;
  otherAssets: string;
  loans: string;
}

/**
 * A projection valued in `currency`. What could not be converted (a holding or a contribution
 * in a currency without a quote) is left out and named in `fxQuality`.
 */
export interface PortfolioProjection {
  currency: string;
  fxQuality: ConversionQuality;
  horizonYears: number;
  viewType: 'monthly' | 'yearly';
  timeline: ProjectionTimelinePoint[];
  projectedNetWorth: string;
  totalContributions: string;
  totalGrowth: string;
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
  balanceAsOf: number | null;
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
  /** Opening balance (create only; the update command ignores it — balances are recorded). */
  balance?: string;
  /** UTC day of the opening balance; with it the create writes the first balance record. */
  balanceAsOf?: number | null;
  institutionId?: string | null;
  interestRate?: string | null;
  hasZoneDesignation?: boolean;
  terminationDate?: number | null;
  /** Overwritten on update like `terminationDate`: send the stored value back, `null` clears it. */
  interestRateValidUntil?: number | null;
  /** Exclude from portfolio balance (for operational/checking accounts) */
  excludeFromBalance?: boolean;
}

/** Financial meaning, independent of native credit/debit direction. */
export type FlowKind = 'income' | 'expense' | 'transfer';

/** Every flow role, in the order pickers list them. */
export const FLOW_KINDS: readonly FlowKind[] = ['income', 'expense', 'transfer'];

export interface BankTransaction {
  /** Null derives meaning from own-account identity, category role and direction. */
  flowKind: FlowKind | null;
  id: string;
  bankAccountId: string;
  transactionId: string | null;
  type: TransactionType;
  amount: string;
  currency: string;
  description: string | null;
  counterpartyName: string | null;
  counterpartyIban: string | null;
  counterpartyBankCode: string | null;
  counterpartyCountry: string | null;
  importBatchId: string | null;
  importFileName: string | null;
  importLine: number | null;
  rowBalance: string | null;
  spendingPlanId: string | null;
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
  /** Null derives meaning from own-account identity, category role and direction. */
  flowKind?: FlowKind | null;
  bankAccountId: string;
  transactionId?: string | null;
  type: TransactionType;
  amount: string;
  currency?: string;
  description?: string | null;
  counterpartyName?: string | null;
  counterpartyIban?: string | null;
  counterpartyBankCode?: string | null;
  counterpartyCountry?: string | null;
  bookingDate: number;
  valueDate?: number | null;
  categoryId?: string | null;
  status?: TransactionStatus;
}

export interface TransactionCategory {
  kind: FlowKind;
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
  kind?: FlowKind;
  name: string;
  icon?: string | null;
  color?: string | null;
  parentId?: string | null;
  sortOrder?: number;
}

/** Edit a category's definition; an absent field is left unchanged. */
export interface UpdateTransactionCategory {
  kind?: FlowKind;
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
  sortBy?: 'date' | 'description' | 'category' | 'amount';
  ascending?: boolean;
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
  counterpartyBankCode?: string | null;
  counterpartyCountry?: string | null;
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

export interface InsertScopedLearning {
  bankAccountId: string;
  isCredit: boolean;
  payee: string | null;
  counterpartyIban: string | null;
  counterpartyBankCode: string | null;
  counterpartyCountry: string | null;
  categoryId: string;
}

/** Learned payee entry for the management UI */
export interface LearnedPayeeEntry {
  bankAccountId: string | null;
  isCredit: boolean | null;
  id: string;
  /** Rule type: "payee_default", "iban_default", "iban_only_default" */
  ruleType: string;
  normalizedPayee: string | null;
  originalPayee: string | null;
  counterpartyIban: string | null;
  categoryId: string;
  createdAt: number;
  updatedAt: number;
}

/** One current-category bucket of the payments a rewritten rule covers (`categoryId` null = no category). */
export interface CategoryCount {
  categoryId: string | null;
  count: number;
}

/** A payment a bulk recategorization changed, with what it held before (for undo). */
export interface ChangedRow {
  id: string;
  previousCategoryId: string | null;
  previousSource: string | null;
}

/** The custom rule that decides a transaction's category today. */
export interface GoverningRule {
  id: string;
  name: string;
  categoryId: string;
}

/** What rewriting the rule behind a transaction would cover. */
export interface RecategorizationPreview {
  /** The layer that decides today. */
  governing: 'ownAccount' | 'custom' | 'learned' | 'pack' | 'none';
  /** Set when `governing` is 'custom'. */
  rule: GoverningRule | null;
  /** False when neither a payee key nor an account identity exists. */
  learnable: boolean;
  /** Other payments the rewritten rule matches, the edited row excluded. */
  matches: number;
  byCategory: CategoryCount[];
}

/** Where a suggested category comes from (the pill's sub-label on the phone). */
export type SuggestionSource = 'mcp' | 'learned' | 'rule' | 'pack';

/** A category the app would give a payment, and where the idea comes from. */
export interface CategorySuggestion {
  categoryId: string;
  source: SuggestionSource;
}

/** One payment of the categorize queue and the category suggested for it, if any. */
export interface CategorizeQueueRow {
  transaction: BankTransaction;
  suggestion: CategorySuggestion | null;
}

/** Booked payments without a category, newest first. */
export interface CategorizeQueue {
  /** The newest payments, capped; `total` counts all of them. */
  rows: CategorizeQueueRow[];
  /** Every payment waiting in the queue (not snoozed), listed or not. */
  total: number;
  /** Uncategorized payments held back from the queue by a snooze that is still running. */
  snoozed: number;
}

/** Hold uncategorized payments back from the queue for a number of days. */
export interface SnoozeCategorization {
  /** 1 to 1000 payment ids. */
  transactionIds: string[];
  /** 1 to 365. */
  days: number;
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

/** Where a bank hands out its statement file on a phone: in its own app, or in its internet banking in the browser. */
export type ExportRoute = 'app' | 'web';

/**
 * How to get a bank's statement file onto a phone (spec §8.6, rung 2) — the `mobileExport` block of a
 * preset. `steps` maps a UI language code to two to four short steps; every language the app ships is
 * present. `appUrlScheme` is a bare scheme (`fio`), `webUrl` an `https://` entry (required for `web`).
 */
export interface MobileExportGuide {
  route: ExportRoute;
  /** What the person picks in the bank ("CSV"; Revolut's personal statement offers "Excel (CSV)"). */
  format: string;
  steps: Record<string, string[]>;
  appUrlScheme: string | null;
  webUrl: string | null;
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
  counterpartyBankCodeColumn: string | null;
  counterpartyCountryColumn: string | null;
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
  /** How to get this bank's statement file on a phone; `null` for the generic layouts. */
  mobileExport: MobileExportGuide | null;
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
  /**
   * 1-based. File line for CSV (the line where the row starts, the header line counts),
   * entry ordinal for statement formats.
   */
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
  promotedCount: number;
  statementBalance: StatementBalance | null;
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

/** One column the content inference assigned to a role. */
export interface InferredField {
  /** The file's own header of the column. */
  column: string;
  /**
   * 0..1: the share of the column's cells that matched the role, 0.1 lower when no header
   * synonym agreed and only the content decided.
   */
  confidence: number;
}

/**
 * What a running balance says about the amounts' signs: `verified` = expenses are negative,
 * `inverted` = the file prints expenses positive, `unchecked` = no balance agrees.
 */
export type SignCheck = 'verified' | 'inverted' | 'unchecked';

/**
 * The roles `infer_csv_columns` assigned to the columns of a CSV no preset describes, from
 * their content (mobile spec §8.6 rung 3). Columns are named by the file's own headers;
 * null = not recognized.
 */
export interface InferredColumns {
  date: InferredField | null;
  /** chrono format of `date`, date part only; null without a date column. */
  dateFormat: string | null;
  /** Day/month order cannot be told from the values; `dateFormat` is the European guess. */
  dateAmbiguous: boolean;
  /** A single signed amount column; null when the file has a debit/credit pair instead. */
  amount: InferredField | null;
  debit: InferredField | null;
  credit: InferredField | null;
  /** A running balance that agrees with the amounts; it is what `signCheck` rests on. */
  balance: InferredField | null;
  signCheck: SignCheck;
  currency: InferredField | null;
  /** IBANs or Czech domestic account numbers of the other party. */
  counterpartyIban: InferredField | null;
  counterparty: InferredField | null;
  transactionId: InferredField | null;
  /** Up to three free-text columns, longest first. */
  description: string[];
  /** "," or ".". */
  decimalSeparator: string;
}

/**
 * How a learned format reads the columns of its file: the layout part of a CSV import
 * configuration, with the file's own header names. Nothing that belongs to one file is kept.
 */
export interface LearnedCsvMapping {
  dateColumn: string;
  /** chrono format, or "auto" to detect it from the column. */
  dateFormat: string;
  amountColumn: string | null;
  debitColumn: string | null;
  creditColumn: string | null;
  balanceColumn: string | null;
  transactionIdColumn: string | null;
  descriptionColumns: string[];
  counterpartyColumn: string | null;
  counterpartyIbanColumn: string | null;
  counterpartyBankCodeColumn: string | null;
  counterpartyCountryColumn: string | null;
  currencyColumn: string | null;
  /** "auto", "," or ".". */
  decimalSeparator: string;
}

/**
 * A CSV layout the person confirmed once, recognised next month (mobile spec §8.6 rung 4).
 * Stored in the profile database; `signature` is the layout's key (header cells, delimiter and
 * column count), `headers` the cells it was learned from. Encoding and header row are what the
 * file had when it was saved and are not part of the key.
 */
export interface LearnedCsvFormat {
  /** "learned:<uuid>". */
  id: string;
  name: string;
  signature: string;
  headers: string[];
  /** One character: "," ";" or a tab. */
  delimiter: string;
  encoding: string;
  /** 0-based line of the header row. */
  headerRow: number;
  mapping: LearnedCsvMapping;
  /** The account the layout was learned for, offered again on reuse. */
  accountId: string | null;
  /** Unix seconds. */
  createdAt: number;
  /** Unix seconds of the last successful import with this format. */
  lastUsedAt: number;
  /** The shipped preset whose headers this layout now matches; a preset always wins. */
  shadowedByPresetId: string | null;
}

/** What `save_learned_csv_format` takes. */
export interface InsertLearnedCsvFormat {
  name: string;
  headers: string[];
  delimiter: string;
  encoding: string;
  headerRow: number;
  mapping: LearnedCsvMapping;
  accountId: string | null;
}

export interface CsvImportConfigInput {
  /** One character; empty = auto-detect. */
  delimiter: string;
  /** Data rows to skip after the header row. */
  skipRows?: number;
  /** 0-based line of the header row; omit to auto-detect. */
  headerRow?: number | null;
  dateColumn: string;
  counterpartyBankCodeColumn?: string | null;
  counterpartyCountryColumn?: string | null;
  /** chrono format, parsed strictly; "" or "auto" detects it. */
  dateFormat: string;
  /** Single signed amount column; optional when a debit/credit pair is given. */
  amountColumn?: string | null;
  /** Debit/credit pair (amount = credit - debit); used when `amountColumn` is not set. */
  debitColumn?: string | null;
  creditColumn?: string | null;
  /** Running balance retained as row provenance and dated statement balance. */
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
  /** Skip possible composite matches (default true); certain bank IDs always skip. */
  skipDuplicates?: boolean;
  /**
   * Row `line`s (file line for CSV, entry ordinal for statement formats) of possible
   * composite duplicates to import anyway.
   */
  importAnywayLines?: number[];
  /** Force a text encoding (`Windows-1250`); omit to detect. */
  encoding?: string | null;
  /**
   * Categories picked in the preview, by row `line` (file line for CSV, entry ordinal for
   * statement formats); applied before the rules on import.
   */
  categoryOverrides?: CsvCategoryOverride[];
  previewOffset?: number;
  previewLimit?: number;
}

/** One row's category chosen in the preview (row `line` → category). */
export interface CsvCategoryOverride {
  /** 1-based. File line for CSV, entry ordinal for statement formats. */
  line: number;
  categoryId: string;
}

/** A closing balance the source declares, in its own currency; `asOf` is a calendar day (UTC midnight). */
export interface DeclaredBalance {
  amount: string;
  currency: string;
  asOf: number;
}

/** The category chosen for one source category of a statement (QIF `L` lines). */
export interface SourceCategoryMapping {
  source: string;
  categoryId: string;
}

/**
 * ok: will be imported. error: cannot be imported (see `message`). duplicate: matches an
 * stored occurrence or bank ID; only possible composites allow `importAnywayLines`.
 * skipped: zero amount or dropped by the row filter.
 * promoted: existing same-ID pending transaction will be booked in place.
 */
export type CsvRowStatus = 'ok' | 'error' | 'duplicate' | 'skipped' | 'promoted';

export interface CsvPreviewRow {
  counterpartyIban: string | null;
  counterpartyBankCode: string | null;
  counterpartyCountry: string | null;
  transactionId: string | null;
  rowBalance: string | null;
  canImportAnyway: boolean;

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
  /** The category the file itself names for the row (QIF `L` line); null for other formats. */
  sourceCategory: string | null;
}

/** An earlier import of the account whose period contains new rows of this file. */
export interface ImportOverlapBatch {
  fileName: string;
  dateRange: CsvDateRange;
}

/** New rows that fall inside the period of an earlier import (their other-format twins may not be recognized as duplicates). */
export interface ImportOverlap {
  /** New rows dated inside an earlier batch's period, its last day excluded. */
  newRowsInCoveredRange: number;
  /** The earlier batches holding such rows, newest first, at most three. */
  batches: ImportOverlapBatch[];
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
  promotedCount: number;
  forcedDuplicateCount: number;
  /** Set when new rows fall inside the period of an earlier import. */
  overlap: ImportOverlap | null;
  offset: number;
  limit: number;
}

export interface CsvImportBatch {
  id: string;
  bankAccountId: string;
  fileName: string;
  importedCount: number;
  duplicateCount: number;
  errorCount: number;
  importedAt: number;
  linkedCount: number;
  promotedCount: number;
  dateRange: CsvDateRange | null;
}

// ----------------------------------------------------------------------------
// Statement import (CAMT.053, MT940, OFX/QFX, QIF)
// ----------------------------------------------------------------------------

export type StatementFormat = 'camt053' | 'mt940' | 'ofx' | 'qif';

/** Order of the day, month and year parts of a QIF date. */
export type DateOrder = 'dmy' | 'mdy' | 'ymd';

/** Whether the declared balances agree with the entries. */
export type BalanceCheck = 'ok' | 'mismatch' | 'unavailable';

/** How the account the file names relates to the selected account. */
export type AccountMatch = 'selected' | 'other' | 'none' | 'unknown';

/** How many entries of a statement carry one source category (QIF `L` line). */
export interface SourceCategoryCount {
  name: string;
  count: number;
}

/** A note about a file that does not stop the import: `key` is a `statementImport.*` i18n key. */
export interface StatementWarning {
  key: string;
  detail: string | null;
}

export interface StatementSummary {
  /** Position among the file's statements; the `statementIndex` of the options. */
  index: number;
  accountIdentifier: string | null;
  accountName: string | null;
  currency: string | null;
  period: CsvDateRange | null;
  openingBalance: DeclaredBalance | null;
  closingBalance: DeclaredBalance | null;
  /** Entries that would be imported. */
  entryCount: number;
  /** Entries shown but never imported (not booked, corrections). */
  skippedCount: number;
  balanceCheck: BalanceCheck;
  /** Declared minus computed, in the statement's currency; set on a mismatch. */
  balanceDifference: string | null;
  accountMatch: AccountMatch;
  matchedAccountId: string | null;
  /** Source categories of the importable entries, most frequent first. */
  sourceCategories: SourceCategoryCount[];
}

/** What `inspect_statement_file` learned about a statement file. */
export interface StatementInspection {
  format: StatementFormat;
  /** Encoding the file was decoded with. */
  encoding: string;
  statements: StatementSummary[];
  /** The first statement of the selected account, else the first. */
  defaultStatementIndex: number;
  /** QIF: the date order cannot be told from the dates. */
  dateOrderAmbiguous: boolean;
  detectedDateOrder: DateOrder | null;
  warnings: StatementWarning[];
}

/** Options of one statement preview or import. */
export interface StatementImportOptionsInput {
  statementIndex: number;
  /** QIF only: how to read the dates; omit for the detected order. */
  dateOrder?: DateOrder | null;
  /** Skip possible composite matches (default true); certain bank IDs always skip. */
  skipDuplicates?: boolean;
  /** Entry ordinals of possible composite duplicates to import anyway. */
  importAnywayLines?: number[];
  /** Categories picked in the preview, by entry ordinal; applied before the rules on import. */
  categoryOverrides?: CsvCategoryOverride[];
  /** Categories chosen for the source categories of the file. */
  sourceCategoryMap?: SourceCategoryMapping[];
  previewOffset?: number;
  previewLimit?: number;
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

/** One line of the performance chart: a set of held positions. */
export interface PerformanceSeriesSpec {
  /** Caller-chosen identifier, echoed in the response; unique within a request. */
  key: string;
  /** The positions of the series. Ids of positions that are not held are ignored. */
  investmentIds: string[];
  /**
   * Compute this series in another currency (the native return of a currency group); it then
   * has no contribution.
   */
  currency?: string | null;
}

/** Request of the performance report: the period, the currency and the series to chart. */
export interface StockPerformanceRequest {
  /** First day (unix seconds, UTC midnight); clamped to the first recorded day. */
  fromTs: number;
  /** Last day (unix seconds, UTC midnight); clamped to the last recorded day. */
  toTs: number;
  /** The currency values, flows and returns are converted to, each at its own day. */
  reportCurrency: string;
  series: PerformanceSeriesSpec[];
}

/** The chart data of one requested series, on the dates of the report. */
export interface PerformanceSeries {
  key: string;
  /** Cumulative time-weighted return of the series' own positions in percent per date (first 0). */
  twr: number[];
  /**
   * Percentage points of the whole-portfolio return the series brought, cumulative per date;
   * null for a series computed in another currency.
   */
  contribution: number[] | null;
  /**
   * Money the series' positions gained over the whole period in the report currency (the value
   * change without buys and sells), decimal TEXT; null for a series computed in another currency.
   */
  gain: string | null;
}

/** One held position over the whole period. */
export interface PositionPerformance {
  investmentId: string;
  /** Time-weighted return of the position over the period, in percent. */
  twr: number;
  /** Percentage points of the whole-portfolio return the position brought. */
  contribution: number;
  /** Value on the first date, decimal TEXT; null when it cannot be converted. */
  startValue: string | null;
  /** Value on the last date, decimal TEXT; null when it cannot be converted. */
  endValue: string | null;
  /** Money the position gained over the period (the value change without buys and sells), decimal TEXT. */
  gain: string;
}

/**
 * Performance of the held positions in `currency`: the whole portfolio, the requested series and
 * every position, all on the same `dates`.
 */
export interface StockPerformance {
  /** The report currency. */
  currency: string;
  fxQuality: ConversionQuality;
  /** Emitted days as `YYYY-MM-DD`, at most 400 plus the last day (every n-th day on long ranges). */
  dates: string[];
  /** Cumulative time-weighted return of the whole portfolio in percent per date (first 0). */
  portfolio: number[];
  /** Portfolio value per date (decimal TEXT); null when no held position converts that day. */
  values: (string | null)[];
  /** Net deposits (buys minus sells) cumulated since the first date, decimal TEXT. */
  netFlows: string[];
  /** Money-weighted return from the first date to each date (modified Dietz, cumulative), in percent. */
  mwr: number[];
  series: PerformanceSeries[];
  positions: PositionPerformance[];
}

/**
 * Daily returns of a market index (or any Yahoo ticker) over the period, for the comparison line
 * of the performance chart.
 */
export interface StockBenchmark {
  /** The validated ticker the prices were fetched for. */
  ticker: string;
  /** The report currency the returns are computed in. */
  currency: string;
  fxQuality: ConversionQuality;
  /**
   * Days as `YYYY-MM-DD`, one per calendar day (the last close carries over non-trading days);
   * empty when the ticker has no prices.
   */
  dates: string[];
  /** Cumulative return of the index in percent per date, the first date being 0. */
  twr: number[];
}

/**
 * One recorded day of a ticker's position. Quantity, price and currency are the native facts;
 * `value` is `quantity × price` converted at that day's rate (common-day rule), null when it
 * cannot be converted.
 */
export interface TickerValueHistory {
  ticker: string;
  recordedAt: number;
  quantity: string;
  price: string;
  currency: string;
  value: string | null;
  /**
   * The position's cost basis as of that day in the report currency (every buy at its own
   * day, sells release cost at the running average); null when unknown.
   */
  costBasis: string | null;
  isStale: boolean;
}

/** A ticker's value history in a report currency, oldest day first. */
export interface TickerValueHistoryReport {
  /** The report currency of every `value`. */
  currency: string;
  fxQuality: ConversionQuality;
  points: TickerValueHistory[];
}

export interface StockInvestmentWithTags {
  id: string;
  ticker: string;
  companyName: string;
  quantity: string;
  /** Native weighted average cost, in `averagePriceCurrency`. */
  averagePrice: string;
  averagePriceCurrency: string;
  /** Current price in its own currency; null when unpriced. */
  price: string | null;
  priceCurrency: string | null;
  value: string | null;
  costBasis: string | null;
  unrealizedGain: string | null;
  unrealizedGainPercent: number | null;
  yearlyDividend: string | null;
  /** Sector from the cached quote data; null for ETFs and unknown. */
  sector: string | null;
  tags: StockTag[];
}

export interface StocksAnalysis {
  currency: string;
  fxQuality: ConversionQuality;
  items: StockInvestmentWithTags[];
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

// Payload for the `history-recalculation` event emitted by the background rebuild of a stock or
// crypto ticker's value history (src-tauri/src/services/history_recalc.rs). `running` when the
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
  /** Attachment rows in the archive (format 2); absent in format 1 manifests. */
  attachmentCount?: number;
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

// Milestones ("Co vás čeká") — src-tauri/src/models/milestones.rs
export type MilestoneKind =
  | 'insurance_anniversary'
  | 'insurance_end'
  | 'insurance_payment'
  | 'loan_fixation_end'
  | 'loan_fixation_expired'
  | 'loan_payoff'
  | 'loan_balance_check'
  | 'bond_maturity'
  | 'bond_coupon'
  | 'account_termination'
  | 'savings_rate_end'
  | 'balances_stale'
  | 'backup_stale'
  | 'valuation_stale'
  | 'watch_target'
  | 'stock_split';
export type MilestoneStage = 'now' | 'soon';
export type MilestoneTone = 'action' | 'info';
export type MilestoneState = 'done' | 'snoozed';

export interface Milestone {
  /** Stable occurrence key; the done / snoozed state is stored under it. */
  key: string;
  kind: MilestoneKind;
  stage: MilestoneStage;
  /** `action` = something to do; `info` = nothing to do. */
  tone: MilestoneTone;
  /** Policy, loan, bond, account or property id; the ticker for targets. */
  sourceId: string | null;
  /** Entity name; empty for the backup and balances items. */
  title: string;
  /** UTC day of the event. */
  dueDay: number | null;
  /** Last day to act when it differs from the event (insurance notice deadline). */
  actionDay: number | null;
  /** Day of the last backup, balance update, valuation or balance check. */
  sinceDay: number | null;
  /** Payment, coupon, returned principal or target price (TEXT money). */
  amount: string | null;
  currency: string | null;
  /** Current price of a crossed target. */
  referenceAmount: string | null;
  direction: TargetDirection | null;
  /** Number of stale accounts. */
  count: number | null;
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
  /** Splits of the ticker inside the file's trade range (spec D13); absent in older configs. */
  splits?: StockSplitOverride[];
}

/**
 * `convert`: the file is in pre-split units, its trades dated before the split are converted
 * (quantity × n/d, price × d/n). `keep`: the file is already in today's units. The stored rows
 * are a decision of their own: `StockSplitOverride.applyStored`.
 */
export type StockSplitAction = 'convert' | 'keep';

/** A split of an instrument's ticker and what the import does about it (spec D13). */
export interface StockSplitOverride {
  /** UTC midnight of the ex-date; trades dated before it are pre-split. */
  effectiveDay: number;
  /** Integer as text, e.g. "10" of a 10:1 split. */
  numerator: string;
  /** Integer as text, e.g. "1" of a 10:1 split. */
  denominator: string;
  /** What happens to the file's trades dated before the split. */
  action: StockSplitAction;
  /**
   * Also apply the split to the ticker's stored transactions before the import, when Moony holds
   * it as pending or has no row for it (D14); independent of `action`. The backend reads an
   * absent field as `false`.
   */
  applyStored: boolean;
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
  /** Rows that will be imported converted to post-split units (D13). */
  converted: number;
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
  /** Imported rows converted to post-split units (D13). */
  converted: number;
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

// ============================================================================
// Stock splits (src-tauri/src/services/stock_splits/types.rs) — spec 2026-10-24-stock-splits
// ============================================================================

export type StockSplitStatus = 'pending' | 'applied' | 'dismissed';
export type StockSplitSource = 'yahoo' | 'manual' | 'import';
/** Whether the pre-split transactions are already in post-split units (spec D7). */
export type StockSplitVerdict = 'adjusted' | 'unadjusted' | 'unknown';

export interface StockSplit {
  id: string;
  ticker: string;
  /** UTC midnight of the ex-date. */
  effectiveDay: number;
  /** Integer as text, e.g. "4". */
  numerator: string;
  /** Integer as text, e.g. "1". */
  denominator: string;
  source: StockSplitSource;
  status: StockSplitStatus;
  detectedAt: number;
  resolvedAt: number | null;
  /** Transactions rewritten by the last apply. */
  appliedCount: number | null;
  /** Transactions of the ticker dated before effectiveDay, now. */
  preSplitCount: number;
  /** D7 from stock_value_history; 'unknown' unless pending. */
  verdict: StockSplitVerdict;
}

/** What an apply or revert did; `earliestDay` is the ticker's first transaction day. */
export interface StockSplitApplyResult {
  split: StockSplit;
  rewritten: number;
  earliestDay: number | null;
}

/** One file trade dated before a split, with its D7 verdict (import wizard, spec D12). */
export interface StockSplitCheckTrade {
  /** 1-based line of the file. */
  line: number;
  verdict: StockSplitVerdict;
}

/** A split Yahoo reports after an instrument's first trade in the file (spec D12). */
export interface StockSplitCheckSplit {
  /** UTC midnight of the ex-date. */
  effectiveDay: number;
  /** Integer as text, e.g. "10". */
  numerator: string;
  /** Integer as text, e.g. "1". */
  denominator: string;
  /** D7 over `trades`. */
  verdict: StockSplitVerdict;
  /** The will-import trades dated before the split, in file order. */
  trades: StockSplitCheckTrade[];
  /** The stock_splits row of (ticker, effectiveDay), if any. */
  storedStatus: StockSplitStatus | null;
  /** Stored transactions of the ticker dated before the split (the D14 line). */
  storedPreSplitCount: number;
  /**
   * D7 over those stored transactions as the position page judges them (against the stored
   * value history, cumulative over the ticker's pending splits); `unknown` unless the split is
   * pending or unknown to Moony and such rows exist.
   */
  storedVerdict: StockSplitVerdict;
}

/** The split check of one instrument of the file; `checked: false` when offline (spec D12). */
export interface StockSplitCheck {
  /** StockImportInstrument.key. */
  key: string;
  ticker: string;
  checked: boolean;
  /** Oldest first; empty when not checked. */
  splits: StockSplitCheckSplit[];
}

export type FlowSource = 'explicit' | 'ownAccount' | 'category' | 'direction';
export type FxQuality = 'historical' | 'estimated' | 'missing';
/** What a converted response could not (or only approximately) convert. */
export interface ConversionQuality {
  missingCurrencies: string[];
  missingCount: number;
  estimated: boolean;
}
/** Current quotes: units of each currency per 1 unit of `reference` (derived codes and the reference included). */
export interface FxSnapshot {
  reference: string;
  units: Record<string, number>;
  /** Unix seconds of the last fetch; 0 when no rates are loaded. */
  revision: number;
}
/** Quotes stored for one UTC day (`day` is the UTC-midnight unix timestamp). */
export interface FxDayQuotes {
  day: number;
  units: Record<string, number>;
}
/** Stored daily quotes: each day's `units` are units of a currency per 1 unit of `reference`. */
export interface FxHistoryQuotes {
  reference: string;
  /** Oldest first. */
  days: FxDayQuotes[];
}
export interface ReportQuality {
  missingFxCount: number;
  missingFxCurrencies: string[];
  estimatedFxCount: number;
  observedCompletedMonths: number[];
  partialMonthCount: number;
  unobservedMonthCount: number;
  /** Stored rows with a non-finite amount (legacy data), excluded from totals. */
  invalidAmountCount: number;
}
export interface EffectiveTransactionFlow {
  id: string;
  effectiveFlowKind: FlowKind;
  flowSource: FlowSource;
}
// Budget goal entity
export interface BudgetGoal {
  id: string;
  categoryId: string;
  timeframe: string; // "monthly" | "quarterly" | "yearly"
  amount: string;
  currency: string;
  createdAt: number;
  updatedAt: number;
  /** Month of the version shown as `amount`: in effect now, or the first scheduled one. */
  effectiveMonth: number | null;
  /** Every limit version, oldest first; `amount: null` removes the limit from that month. */
  versions: BudgetGoalVersion[];
}

// One limit version of a budget goal, effective from a calendar month (UTC epoch).
export interface BudgetGoalVersion {
  effectiveMonth: number;
  amount: string | null;
  currency: string;
}

/** Exact configured interval selected for deletion; its boundaries are retained as a gap. */
export interface BudgetGoalInterval {
  categoryId: string;
  timeframe: string;
  effectiveMonth: number;
}

// Data for creating/updating a budget goal
export interface InsertBudgetGoal {
  categoryId: string;
  timeframe: string;
  amount: string;
  currency?: string;
  effectiveMonth?: number;
  /** Last month of the limit (inclusive); afterwards the earlier limit returns. Omitted: open-ended. */
  effectiveUntil?: number;
}

// Category spending summary
export interface CategorySpendingSummary {
  budgetCurrency: string | null;
  budgetSpent: string | null;
  budgetLimitNative: string | null;
  budgetLimit: string | null;
  budgetLimitFxQuality: FxQuality | null;
  categoryId: string;
  categoryName: string;
  categoryIcon: string | null;
  categoryColor: string | null;
  totalAmount: string;
  transactionCount: number;
  budgetGoal: BudgetGoal | null;
  budgetPercentage: number | null;
}

// Transaction for budgeting view
export interface BudgetingTransaction {
  reportCurrency: string;
  effectiveFlowKind: FlowKind;
  flowSource: FlowSource;
  signedAmount: string | null;
  fxQuality: FxQuality;
  id: string;
  bookingDate: number;
  amount: string;
  currency: string;
  description: string | null;
  remittanceInfo: string | null;
  counterpartyName: string | null;
  counterpartyIban: string | null;
  counterpartyBankCode: string | null;
  counterpartyCountry: string | null;
  categoryId: string | null;
  bankAccountId: string;
  bankAccountName: string;
  txType: string;
}

// Full budgeting report
export interface BudgetingReport {
  reportCurrency: string;
  bankCashMovement: string;
  quality: ReportQuality;
  nonconsumptionFlows: CashflowGroup[];
  periodStart: number;
  periodEnd: number;
  timeframe: string;
  totalIncome: string;
  totalExpenses: string;
  netBalance: string;
  incomeCategories: CategorySpendingSummary[];
  expenseCategories: CategorySpendingSummary[];
  uncategorizedIncome: string;
  uncategorizedExpenses: string;
  uncategorizedTransactionCount: number;
  /** Included booked uncategorized expense rows, including unavailable FX. */
  uncategorizedExpenseTransactionCount: number;
}

export type ReportTransaction = BudgetingTransaction;

export interface StatementBalance {
  amount: string;
  currency: string;
  asOf: number;
  line: number;
  batchId: string;
}

export type BalanceRecordSource = 'manual' | 'statement';

/** A dated balance of a bank account; the newest one is mirrored into `BankAccount.balance`. */
export interface BankBalanceRecord {
  id: string;
  bankAccountId: string;
  /** Balance in the account currency. */
  amount: string;
  currency: string;
  /** UTC day start (unix seconds). */
  asOf: number;
  source: BalanceRecordSource;
  sourceBatchId: string | null;
  recordedAt: number;
}

/** Input of `record_bank_account_balance`; the source is derived from `sourceBatchId`. */
export interface InsertBalanceRecord {
  amount: string;
  currency: string;
  asOf: number;
  /** Needed only while the account has no dated record and an undated nonzero balance. */
  confirmUnknownAsOf?: boolean;
  sourceBatchId?: string | null;
}
